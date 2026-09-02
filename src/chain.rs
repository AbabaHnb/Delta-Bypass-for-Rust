//! 从链接一路走到钥匙
//! Walk from a link all the way to a key
//!
//! 整条路是这样的：
//!
//! ```text
//! 链接 → 问情况 → 识别验证码 → 提交 → 解出下一段 → 再提交 → … → 拿到钥匙
//! link → ask about it → picture puzzle → submit → decode next → submit again → … → key
//! ```
//!
//! 三个省时间的做法：
//!
//! 1. **问情况、查钥匙、识别验证码三件事一起干。** 识别要花一秒多，那段时间正好用来问
//!    服务器情况、顺便看看这链接是不是已经完成过了。
//!
//! 2. **一道题走完全程。** 凭证换一次就够，不用每关都重新识别。
//!
//! 3. **提交和查钥匙叠着做。** 提交发出去 50 毫秒后就开始查钥匙，不等提交回话 ——
//!    省掉一趟来回。
//!
//! The whole route looks like this:
//!
//! ```text
//! link → ask about it → picture puzzle → submit → decode next → submit again → … → key
//! ```
//!
//! Three things that save time:
//!
//! 1. **Ask about it, check for the key, and do the puzzle all at once.** The puzzle
//!    takes over a second, and that time is used to ask the server about the link and
//!    to see whether it has already been completed.
//!
//! 2. **One puzzle for the whole route.** Swapping for a token once is enough; there
//!    is no need to redo the puzzle at every checkpoint.
//!
//! 3. **Overlap submitting with checking for the key.** Checking starts 50
//!    milliseconds after the submission goes out, without waiting for its reply — one
//!    round trip saved.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::auth;
use crate::config;
use crate::net;
use crate::pool;
use crate::solver;
use crate::timing::{steps, Timer};

/// 服务编号。上游告诉我们用哪个；问不出来就用这个兜底。
/// The service number. The far end tells us which to use; this is the fallback when it
/// will not say.
const FALLBACK_SERVICE: i64 = 3;

// ---------------------------------------------------------------------------
// 等待余量：自己会调 / The waiting margin: it tunes itself
// ---------------------------------------------------------------------------

/// 当前多留的余量（毫秒）。
///
/// 为什么要留：两次提交必须隔 5 秒，但我们只知道自己什么时候发出去，对面看的是它
/// 什么时候收到。网络快慢有波动，掐着 5 秒发很可能 4.98 秒就到了，对面就判"太快"，
/// 那要罚等 2 秒 —— 比多留 0.2 秒亏多了。
///
/// 怎么调：一直顺就慢慢往下减，被拒一次立刻往上加。这样它会自己找到当前网络下最
/// 合适的值。
///
/// The extra margin currently in use, in milliseconds.
///
/// Why have one: two submissions must be 5 seconds apart, but we only know when we
/// sent ours while the far end goes by when it arrived. Network speed wobbles, so
/// sending at exactly 5 seconds may well arrive at 4.98, the far end calls it "too
/// fast", and we get a 2-second penalty — far worse than an extra 0.2 seconds.
///
/// How it tunes: shrink slowly while things go well, jump up the moment we get
/// refused. That way it finds the right value for the current network by itself.
static MARGIN_MS: AtomicU64 = AtomicU64::new(config::GAP_MARGIN_START.as_millis() as u64);

/// 连续顺利了几次。
/// How many good runs in a row so far.
static GOOD_RUNS: AtomicU64 = AtomicU64::new(0);

/// 现在留多少余量。
/// How much margin to leave right now.
fn margin() -> Duration {
    Duration::from_millis(MARGIN_MS.load(Ordering::Relaxed))
}

/// 这次很顺利。攒够几次就把余量减一点。
/// That went well. After enough good runs, shave the margin a little.
fn note_good() {
    let runs = GOOD_RUNS.fetch_add(1, Ordering::Relaxed) + 1;
    if runs < config::CLEAN_STEPS_TO_RELAX {
        return;
    }

    GOOD_RUNS.store(0, Ordering::Relaxed);

    let now = MARGIN_MS.load(Ordering::Relaxed);
    let less = now
        .saturating_sub(config::GAP_MARGIN_STEP_DOWN.as_millis() as u64)
        .max(config::GAP_MARGIN_MIN.as_millis() as u64);

    MARGIN_MS.store(less, Ordering::Relaxed);
}

/// 被判"太快"了。余量马上加上去，好运计数清零。
/// We were told "too fast". Put the margin up straight away and reset the good-run
/// count.
fn note_too_fast() {
    GOOD_RUNS.store(0, Ordering::Relaxed);

    let now = MARGIN_MS.load(Ordering::Relaxed);
    let more = (now + config::GAP_MARGIN_STEP_UP.as_millis() as u64)
        .min(config::GAP_MARGIN_MAX.as_millis() as u64);

    MARGIN_MS.store(more, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// 换通行凭证 / Getting a pass token
// ---------------------------------------------------------------------------

/// 拿一个通行凭证。
///
/// 先看预备池里有没有现成的 —— 有的话只剩最后一步"换凭证"，快得多。没有就当场从头
/// 做一道。
///
/// 不管哪条路，凭证都是这一刻换的，换完立刻用掉。
///
/// Get a pass token.
///
/// First look for one ready in the store — if there is, only the final swap remains,
/// which is much quicker. If not, do one from scratch on the spot.
///
/// Either way the token is swapped for at this moment and used immediately.
fn get_token(
    client: &reqwest::blocking::Client,
    talk: bool,
    timer: &mut Timer,
) -> Result<String, String> {
    timer.start(steps::CAPTCHA);

    // ---- 快路：用备好的题 ----
    // ---- Quick route: use a ready-made puzzle ----
    if let Some(ready) = pool::take() {
        match pool::swap_for_token(client, &ready) {
            Ok((token, swap_ms)) => {
                timer.stop();
                let left = pool::snapshot();
                say(
                    talk,
                    &format!(
                        "  [验证码] 类型={} 坐标=({:.0},{:.0}) 策略={} 识别={:.0}ms \
                         (来源=预备池 缓存年龄={:.1}s 令牌签发={:.0}ms 池余量={})",
                        ready.kind, ready.x, ready.y, ready.method, ready.took_ms,
                        ready.made_at.elapsed().as_secs_f64(), swap_ms, left.usable
                    ),
                );
                return Ok(token);
            }
            Err(why) => {
                // 这道题不能用了，走当场做的路，别卡在这。
                // This one is no good, so fall through to doing one on the spot rather
                // than getting stuck here.
                say(talk, &format!("  [验证码] 预备条目失效({})，转为即时识别", why));
            }
        }
    }

    // ---- 慢路：当场从头做一道 ----
    // ---- Slow route: do one from scratch on the spot ----
    for round in 0..config::CAPTCHA_MAX_RETRIES {
        let fetch_start = Instant::now();

        let puzzle: Value = match client
            .get(format!("{}/challenge", config::CAPTCHA_API))
            .timeout(config::IMAGE_TIMEOUT)
            .send()
            .and_then(|r| r.json())
        {
            Ok(v) => v,
            Err(e) => {
                say(talk, &format!("  [验证码] 获取挑战失败: {}", e));
                break;
            }
        };
        let fetch_ms = fetch_start.elapsed().as_secs_f64() * 1000.0;

        let picture_path = puzzle.get("image").and_then(Value::as_str).unwrap_or("");
        let kind = puzzle.get("type").and_then(Value::as_str).unwrap_or("");

        let download_start = Instant::now();
        let picture = match client
            .get(format!("{}{}", config::CAPTCHA_HOST, picture_path))
            .header("Referer", format!("{}/", config::CAPTCHA_HOST))
            .timeout(config::IMAGE_TIMEOUT)
            .send()
            .and_then(|r| r.bytes())
        {
            Ok(b) => b.to_vec(),
            Err(e) => {
                say(talk, &format!("  [验证码] 下载图像失败: {}", e));
                break;
            }
        };
        let download_ms = download_start.elapsed().as_secs_f64() * 1000.0;

        let solve_start = Instant::now();
        let (choice, method) = solver::solve(&picture, kind);
        let solve_ms = solve_start.elapsed().as_secs_f64() * 1000.0;

        let Some((x, y)) = choice else {
            say(
                talk,
                &format!("  [验证码] 第{}次尝试: 识别未命中({:.0}ms)，重新获取挑战", round + 1, solve_ms),
            );
            continue;
        };

        let swap_start = Instant::now();
        let reply: Value = match client
            .post(format!("{}/answer", config::CAPTCHA_API))
            .json(&json!({ "challenge_id": puzzle["challenge_id"], "x": x, "y": y }))
            .timeout(config::IMAGE_TIMEOUT)
            .send()
            .and_then(|r| r.json())
        {
            Ok(v) => v,
            Err(e) => {
                say(talk, &format!("  [验证码] 提交答案失败: {}", e));
                break;
            }
        };
        let swap_ms = swap_start.elapsed().as_secs_f64() * 1000.0;

        if reply.get("success").and_then(Value::as_bool) == Some(true) {
            let token = reply
                .get("token")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            timer.stop();
            say(
                talk,
                &format!(
                    "  [验证码] 类型={} 坐标=({:.0},{:.0}) 策略={} 识别={:.0}ms \
                     (获取挑战={:.0}ms 下载图像={:.0}ms 令牌签发={:.0}ms)",
                    kind, x, y, method, solve_ms, fetch_ms, download_ms, swap_ms
                ),
            );
            return Ok(token);
        }

        say(
            talk,
            &format!("  [验证码] 第{}次尝试: 服务端校验未通过({:.0}ms)，重新获取挑战", round + 1, solve_ms),
        );
    }

    timer.stop();
    Err(format!(
        "验证码识别失败(共尝试{}次) / Captcha recognition failed after {} attempts",
        config::CAPTCHA_MAX_RETRIES, config::CAPTCHA_MAX_RETRIES
    ))
}

/// verbose 模式下才打印。
/// Only print when talking.
fn say(talk: bool, line: &str) {
    if talk {
        println!("{}", line);
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
}

// ---------------------------------------------------------------------------
// 问链接情况 / Asking about the link
// ---------------------------------------------------------------------------

/// 问出来的链接情况。
/// What we learned about the link.
struct LinkInfo {
    /// 用哪个服务编号。问不出来就是空。
    /// Which service number to use. Empty if it would not say.
    service: Option<i64>,
    /// 要过几关。问不出来就是空。
    /// How many checkpoints there are. Empty if it would not say.
    checkpoints: Option<i64>,
    /// 这链接还能不能用。
    /// Whether the link is still usable.
    alive: bool,
    /// 不能用的原因。
    /// Why it is not usable.
    dead_reason: Option<String>,
}

/// 问服务器这个链接的情况。
///
/// 一次问清三件事：用哪个服务、要过几关、还有没有效。原来要问两次，合成一次省一趟
/// 来回。
///
/// Ask the server about this link.
///
/// Three things in one go: which service, how many checkpoints, and whether it is
/// still valid. This used to take two questions; merging them saves a round trip.
fn ask_about(ticket: &str, talk: bool) -> LinkInfo {
    let reply = auth::session_info(ticket);

    // 先看有没有明确说"这链接废了"。
    // First see whether it plainly says the link is dead.
    let said_no = reply.get("success").and_then(Value::as_bool) == Some(false);
    let just_a_hiccup = reply.get("transient").and_then(Value::as_bool) == Some(true);

    if said_no && !just_a_hiccup {
        let words = reply
            .get("message")
            .or_else(|| reply.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let lower = words.to_lowercase();

        if auth::DEAD_LINK_WORDS.iter().any(|w| lower.contains(w)) {
            say(talk, &format!("  [会话] 链接已失效: {}", words));
            return LinkInfo {
                service: None,
                checkpoints: None,
                alive: false,
                dead_reason: Some(words.to_string()),
            };
        }
    }

    // 再从回话里把服务编号和关卡数掏出来。
    // Then dig the service number and checkpoint count out of the reply.
    let body = reply.get("data").unwrap_or(&reply);

    if let Some(profile) = body
        .get("activeRevenueProfile")
        .and_then(Value::as_object)
    {
        if let Some(service) = profile.get("service").and_then(Value::as_i64) {
            let checkpoints = profile
                .get("checkpointCount")
                .and_then(Value::as_i64)
                .filter(|&n| n > 0);

            let hours = body.get("duration").and_then(Value::as_str).unwrap_or("?");
            say(
                talk,
                &format!(
                    "  [会话] 服务ID={} 检查点数={} 有效期={}小时",
                    service,
                    checkpoints.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
                    hours
                ),
            );

            return LinkInfo {
                service: Some(service),
                checkpoints,
                alive: true,
                dead_reason: None,
            };
        }
    }

    // 什么都没问出来。当能用处理，别把好链接判死。
    // Nothing came out. Treat it as usable rather than writing off a good link.
    LinkInfo {
        service: None,
        checkpoints: None,
        alive: true,
        dead_reason: None,
    }
}

// ---------------------------------------------------------------------------
// 查钥匙 / Checking for the key
// ---------------------------------------------------------------------------

/// 查一次钥匙好了没。
/// Check once whether the key is ready.
fn check_key(ticket: &str, talk: bool, timer: Option<&mut Timer>) -> Option<String> {
    let mut timer = timer;

    if let Some(t) = timer.as_mut() {
        t.start(steps::POLL);
    }
    let reply = auth::session_status(ticket);
    if let Some(t) = timer.as_mut() {
        t.stop();
    }

    let body = reply.get("data").unwrap_or(&reply);    let key = body.get("key").and_then(Value::as_str).unwrap_or("");

    // 服务器用 KEY_NOT_FOUND 表示"还没好"，那不是钥匙。
    // The server uses KEY_NOT_FOUND to mean "not ready yet", which is not a key.
    if key.is_empty() || key == "KEY_NOT_FOUND" {
        return None;
    }

    say(talk, &format!("  [密钥] 已获取: {}", key));
    Some(key.to_string())
}

/// 反复查钥匙，拿到就返回。
/// Check for the key repeatedly, returning as soon as it turns up.
fn wait_for_key(ticket: &str, talk: bool, timer: &mut Timer) -> Option<String> {
    for round in 0..config::POLL_MAX_ATTEMPTS {
        if let Some(key) = check_key(ticket, talk, Some(timer)) {
            return Some(key);
        }
        if round + 1 < config::POLL_MAX_ATTEMPTS {
            std::thread::sleep(config::POLL_INTERVAL);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 提交 / Submitting
// ---------------------------------------------------------------------------

/// 看服务器是不是在说"你太快了"。
/// See whether the server is saying "you are going too fast".
fn says_too_fast(reply: &Value) -> bool {
    let mut all = String::new();
    for field in ["message", "error", "detail"] {
        if let Some(text) = reply.get(field).and_then(Value::as_str) {
            all.push_str(text);
            all.push(' ');
        }
    }
    let lower = all.to_lowercase();
    lower.contains("too fast") || lower.contains("slow down") || lower.contains("too many")
}

/// 提交一次的结果。
/// The outcome of one submission.
struct SubmitResult {
    /// 成功时用的是哪个服务编号。失败是空。
    /// Which service number worked. Empty on failure.
    service: Option<i64>,
    /// 服务器的原始回话。
    /// The server's reply as-is.
    reply: Value,
    /// 叠着查钥匙时顺手拿到的钥匙。
    /// A key picked up by the overlapping check, if any.
    key_found: Option<String>,
    /// 那个查钥匙的活儿还在跑，从这里取结果，不用另开一趟。
    /// The check is still running; take its result from here rather than starting a
    /// fresh one.
    key_channel: Option<mpsc::Receiver<Option<String>>>,
}

/// 提交一次，往前推一关。
///
/// 里面做了三件事：
/// 1. 等够 5 秒（从上次**发出**算起，不是从上次收到回话算起 —— 省一趟来回）
/// 2. 提交，被判太快就退避重试
/// 3. 同时叠着查钥匙（`overlap` 为真时）
///
/// Submit once, to move one checkpoint forward.
///
/// Three things happen inside:
/// 1. Wait out the 5 seconds (measured from when we last **sent**, not from when its
///    reply arrived — one round trip saved)
/// 2. Submit, backing off and retrying if told we are too fast
/// 3. Check for the key at the same time (when `overlap` is true)
fn submit_once(
    ticket: &str,
    token: &str,
    service: Option<i64>,
    talk: bool,
    timer: &mut Timer,
    last_sent: &mut Option<Instant>,
    overlap: bool,
) -> SubmitResult {
    // 服务编号：优先用问出来的，不行就用兜底的。
    // Service numbers: prefer the one we were told, fall back otherwise.
    let mut to_try: Vec<i64> = Vec::new();
    if let Some(s) = service {
        to_try.push(s);
    }
    if !to_try.contains(&FALLBACK_SERVICE) {
        to_try.push(FALLBACK_SERVICE);
    }

    timer.start(steps::STEP);

    // ---- 等够间隔 ----
    // ---- Wait out the gap ----
    //
    // 这 5 秒是对面的硬规定，不能缩。但从"上次发出"算起而不是"上次收到"算起，
    // 就能把那趟来回的时间算在等待里，白省 0.2 秒。
    //
    // The 5 seconds is the far end's hard rule and cannot be shortened. But measuring
    // from "when we last sent" rather than "when its reply came back" counts that round
    // trip as part of the wait, saving 0.2 seconds for free.
    let need_to_wait = config::MIN_STEP_GAP + margin();
    if let Some(sent_at) = last_sent.as_ref() {
        let waited = sent_at.elapsed();
        if waited < need_to_wait {
            let more = need_to_wait - waited;
            say(
                talk,
                &format!(
                    "  [提交] 距上次发送{:.2}s，需补足{:.2}s(自适应余量{}ms)",
                    waited.as_secs_f64(),
                    more.as_secs_f64(),
                    margin().as_millis()
                ),
            );
            std::thread::sleep(more);
        }
    }

    let mut key_channel: Option<mpsc::Receiver<Option<String>>> = None;
    let mut checker_started = false;
    let found_key = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));

    for service_number in to_try {
        for attempt in 0..=config::STEP_THROTTLE_RETRIES {
            // ---- 叠着查钥匙 ----
            // ---- Start the overlapping check ----
            //
            // 提交发出去 50 毫秒后就开查，不等它回话。要是钥匙已经好了，这趟就白赚。
            //
            // Start checking 50ms after the submission goes out, without waiting for
            // its reply. If the key is already there, this trip comes free.
            if overlap && !checker_started {
                checker_started = true;

                let (sender, receiver) = mpsc::channel();
                key_channel = Some(receiver);

                let ticket = ticket.to_string();
                let shared = found_key.clone();

                std::thread::spawn(move || {
                    std::thread::sleep(config::POLL_OVERLAP_DELAY);

                    for _ in 0..config::POLL_MAX_ATTEMPTS {
                        let reply = auth::session_status(&ticket);
                        let body = reply.get("data").unwrap_or(&reply);
                        let key = body.get("key").and_then(Value::as_str).unwrap_or("");

                        if !key.is_empty() && key != "KEY_NOT_FOUND" {
                            *shared.lock().unwrap() = Some(key.to_string());
                            let _ = sender.send(Some(key.to_string()));
                            return;
                        }

                        std::thread::sleep(config::POLL_INTERVAL);
                    }

                    let _ = sender.send(None);
                });
            }

            // ---- 提交 ----
            // ---- Submit ----
            let sent_at = Instant::now();
            *last_sent = Some(sent_at);

            let reply = auth::submit(ticket, token, service_number);
            let took_ms = sent_at.elapsed().as_secs_f64() * 1000.0;

            // 花的时间明显偏长，说明里面重试过，真正最后一次发出比 sent_at 晚。
            // 那就改用"现在"当发出时刻，宁可下次多等一点也不能少等。
            //
            // Taking noticeably long means it retried inside, so the real last send was
            // later than sent_at. Use "now" instead — better to wait a little too long
            // next time than not long enough.
            if sent_at.elapsed() > Duration::from_millis(1000) {
                *last_sent = Some(Instant::now());
            }

            if reply.get("success").and_then(Value::as_bool) == Some(true) {
                timer.stop();
                note_good();
                say(talk, &format!("  [提交] 服务ID={} 成功({:.0}ms)", service_number, took_ms));

                let key_found = found_key.lock().unwrap().clone();

                return SubmitResult {
                    service: Some(service_number),
                    reply,
                    key_found,
                    key_channel,
                };
            }

            // 被判太快：余量加上去，歇一会儿再试。
            // Told we are too fast: put the margin up and rest before trying again.
            if says_too_fast(&reply) {
                note_too_fast();

                if attempt < config::STEP_THROTTLE_RETRIES {
                    say(
                        talk,
                        &format!(
                            "  [提交] 服务ID={} 触发频率限制，退避{}s后重试({}/{})",
                            service_number,
                            config::STEP_THROTTLE_SLEEP.as_secs(),
                            attempt + 1,
                            config::STEP_THROTTLE_RETRIES
                        ),
                    );
                    std::thread::sleep(config::STEP_THROTTLE_SLEEP);
                    continue;
                }
            }

            let why = reply.to_string();
            say(
                talk,
                &format!(
                    "  [提交] 服务ID={} 失败: {} ({:.0}ms)",
                    service_number,
                    &why[..why.len().min(200)],
                    took_ms
                ),
            );
            break;
        }
    }

    timer.stop();

    // 先把钥匙取出来存到变量里再拼结构，不然临时借用活不到函数返回。
    // Pull the key into a variable before building the result, otherwise the temporary
    // borrow does not live long enough to return.
    let key_found = found_key.lock().unwrap().clone();

    SubmitResult {
        service: None,
        reply: json!({ "success": false, "error": "全部服务ID均提交失败 / All service IDs failed" }),
        key_found,
        key_channel: None,
    }
}

// ---------------------------------------------------------------------------
// 主流程 / The main route
// ---------------------------------------------------------------------------

/// 走完一条链接的结果。
/// The outcome of working through one link.
pub struct Outcome {
    /// 拿到的钥匙。没拿到就是空。
    /// The key we got. Empty if we did not get one.
    pub key: Option<String>,
    /// 各步骤耗时。
    /// How long each step took.
    pub timer: Timer,
    /// 没拿到的话，是在哪一步停下的。看日志时有用。
    /// If we did not get one, where it stopped. Handy when reading logs.
    pub stopped_at: &'static str,
}
/// 从一条链接一路走到钥匙。
///
/// `talk` 为真会一路打印进度。`max_rounds` 是起始轮数上限，实际会按服务器说的关卡
/// 数往上调。
///
/// Work from one link all the way to a key.
///
/// With `talk` true it prints progress along the way. `max_rounds` is a starting cap,
/// and it gets raised to match however many checkpoints the server reports.
pub fn run(ticket: &str, talk: bool, max_rounds: usize) -> Outcome {
    let mut ticket = ticket.to_string();
    let mut timer = Timer::new();
    let mut rounds_allowed = max_rounds.max(1);
    let mut round = 0usize;
    let mut stopped_at = "已达轮数上限 / Round limit reached";
    let mut last_sent: Option<Instant> = None;
    let mut token: Option<String> = None;

    // 共用的连接。出连接层问题才换。
    // The shared connection. Only swapped if something goes wrong at that level.
    let mut client = net::captcha_client();

    while round < rounds_allowed {
        say(talk, &format!("  [第 {}/{} 轮]", round + 1, rounds_allowed));
        timer.start(steps::META);

        // ---- 三件事一起干 ----
        // ---- Three things at once ----
        //
        // 识别验证码要一秒多，这段时间正好用来问链接情况。第一轮还顺便查一下钥匙 ——
        // 万一这链接之前已经走完了，就能直接返回，一秒都不用花。
        //
        // The puzzle takes over a second, and that time is used to ask about the link.
        // On the first round we also check for the key — if this link was already
        // finished before, we can return straight away without spending a second.
        let (info_sender, info_receiver) = mpsc::channel();
        let (key_sender, key_receiver) = mpsc::channel();

        let info_ticket = ticket.clone();
        let info_worker = std::thread::spawn(move || {
            let _ = info_sender.send(ask_about(&info_ticket, talk));
        });

        let key_worker = if round == 0 {
            let key_ticket = ticket.clone();
            Some(std::thread::spawn(move || {
                let _ = key_sender.send(check_key(&key_ticket, talk, None));
            }))
        } else {
            None
        };

        // 凭证换一次就够，整条路都用它。
        // One token is enough; it serves the whole route.
        if token.is_none() {
            match get_token(&client, talk, &mut timer) {
                Ok(t) => token = Some(t),
                Err(why) => {
                    say(talk, &format!("  [错误] {}", why));
                    say(talk, "  [重试] 重建连接后再次识别");

                    // 有可能是连接坏了，换一条重来。
                    // The connection may have gone bad, so swap it and try again.
                    net::reset_captcha_client();
                    client = net::captcha_client();

                    match get_token(&client, talk, &mut timer) {
                        Ok(t) => token = Some(t),
                        Err(why2) => {
                            say(talk, &format!("  [错误] {}", why2));
                            say(talk, "  [终止] 验证码识别连续失败，无法继续");

                            let _ = info_worker.join();
                            if let Some(w) = key_worker {
                                let _ = w.join();
                            }

                            return Outcome {
                                key: None,
                                timer,
                                stopped_at: "验证码识别失败 / Captcha recognition failed",
                            };
                        }
                    }
                }
            }
        }

        // ---- 第一轮：看看是不是已经完成过了 ----
        // ---- First round: see whether it was already finished ----
        if let Some(worker) = key_worker {
            if let Ok(Some(key)) = key_receiver.recv_timeout(Duration::from_secs(6)) {
                say(talk, "  [会话] 该链接此前已完成，直接返回既有密钥");
                let _ = worker.join();
                let _ = info_worker.join();

                return Outcome {
                    key: Some(key),
                    timer,
                    stopped_at: "链接此前已完成 / Link was already completed",
                };
            }
            let _ = worker.join();
        }

        // ---- 收下链接情况 ----
        // ---- Take in what we learned about the link ----
        //
        // 每轮都重新问一次，所以服务编号也每轮重新拿，不用跨轮记着。
        // Each round asks again, so the service number is taken fresh each round rather
        // than remembered across rounds.
        let mut service: Option<i64> = None;

        match info_receiver.recv_timeout(Duration::from_secs(6)) {
            Ok(info) => {
                service = info.service;

                // 明确说废了就别浪费时间了。
                // Plainly dead, so stop wasting time.
                if !info.alive {
                    timer.invalid_reason = info.dead_reason.clone();
                    say(
                        talk,
                        &format!("  [终止] 链接已失效: {}", info.dead_reason.unwrap_or_default()),
                    );
                    let _ = info_worker.join();

                    return Outcome {
                        key: None,
                        timer,
                        stopped_at: "链接已失效 / Link is invalid",
                    };
                }

                // 关卡数比预设多，就把轮数放宽。留一轮余量，因为最后一关之后还要取钥匙。
                // More checkpoints than assumed, so loosen the round cap. One spare
                // round is added, since the key still has to be fetched after the last
                // checkpoint.
                if let Some(count) = info.checkpoints {
                    let need = (count + 1) as usize;
                    if need > rounds_allowed {
                        rounds_allowed = need.min(config::MAX_ROUNDS_HARD_CAP);
                        say(
                            talk,
                            &format!("  [轮数] 检查点数={}，上限调整为{}轮", count, rounds_allowed),
                        );
                    }
                }
            }
            Err(_) => {
                // 问不出来就往下走，用兜底服务编号。
                // Nothing came back, so carry on with the fallback service number.
                let _ = info_worker.join();
            }
        }

        timer.stop();

        // ---- 接下来要闲 5 秒，趁这段时间叫后台补题 ----
        // ---- Five idle seconds coming up, so nudge the background to top up ----
        if round > 0 {
            pool::nudge();
        }

        // ---- 提交 ----
        // ---- Submit ----
        //
        // 第一轮不叠着查钥匙 —— 才刚开始，钥匙不可能好，白查一趟。
        // No overlapping check on the first round — it has only just begun, the key
        // cannot possibly be ready, and the trip would be wasted.
        let overlap = round > 0;

        let result = submit_once(
            &ticket,
            token.as_deref().unwrap_or(""),
            service,
            talk,
            &mut timer,
            &mut last_sent,
            overlap,
        );

        // 叠着查的那趟已经拿到钥匙了，直接结束。
        // The overlapping check already got the key, so we are done.
        if let Some(key) = result.key_found {
            say(talk, &format!("  [密钥] 已获取(并发轮询): {}", key));
            return Outcome {
                key: Some(key),
                timer,
                stopped_at: "成功 / Success",
            };
        }

        let Some(_worked) = result.service else {
            say(talk, &format!("  [跳过] 第{}轮提交全部失败", round + 1));
            stopped_at = "提交失败 / Submission failed";
            round += 1;
            continue;
        };

        // ---- 看服务器让我们去哪 ----
        // ---- See where the server is sending us ----
        let next_url = result
            .reply
            .get("data")
            .and_then(|d| d.get("url"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        if next_url.is_empty() {
            say(talk, "  [跳过] 响应中缺少跳转地址");
            stopped_at = "响应缺少地址 / Response missing address";
            round += 1;
            continue;
        }

        let shown = if next_url.len() > 80 {
            format!("{}...", &next_url[..80])
        } else {
            next_url.clone()
        };
        say(talk, &format!("  [跳转] {}", shown));

        // ---- 情况一：about:blank 表示走完了，去取钥匙 ----
        // ---- Case one: about:blank means we are through, so fetch the key ----
        if next_url == "about:blank" {
            // 那个叠着查的活儿还在跑，直接等它的结果，别另开一趟。
            // The overlapping check is still running, so wait on its result rather than
            // starting a fresh trip.
            if let Some(channel) = result.key_channel {
                timer.start(steps::POLL);
                let waited = channel.recv_timeout(
                    config::POLL_INTERVAL * config::POLL_MAX_ATTEMPTS as u32
                        + Duration::from_secs(2),
                );
                timer.stop();

                if let Ok(Some(key)) = waited {
                    say(talk, &format!("  [密钥] 已获取(并发轮询): {}", key));
                    return Outcome {
                        key: Some(key),
                        timer,
                        stopped_at: "成功 / Success",
                    };
                }
            }

            say(
                talk,
                &format!(
                    "  [轮询] 流程已完成，最多查询{}次，间隔{:.1}s",
                    config::POLL_MAX_ATTEMPTS,
                    config::POLL_INTERVAL.as_secs_f64()
                ),
            );

            if let Some(key) = wait_for_key(&ticket, talk, &mut timer) {
                return Outcome {
                    key: Some(key),
                    timer,
                    stopped_at: "成功 / Success",
                };
            }

            say(talk, &format!("  [跳过] 查询{}次后密钥仍未就绪", config::POLL_MAX_ATTEMPTS));
            stopped_at = "密钥未就绪 / Key never became ready";
            round += 1;
            continue;
        }

        // ---- 情况二：广告页链接，从里面解出下一段通行串 ----
        // ---- Case two: an advert page link, with the next pass string inside ----
        if let Some(callback) = auth::decode_callback(&next_url) {
            if let Some(next_ticket) = auth::ticket_from_callback(&callback) {
                // 太短的不像真的通行串，别当真。
                // Anything short is not a real pass string, so do not take it seriously.
                if next_ticket.len() > 50 {
                    say(
                        talk,
                        &format!(
                            "  [下一段] {}... (长度={})",
                            &next_ticket[..next_ticket.len().min(24)],
                            next_ticket.len()
                        ),
                    );
                    ticket = next_ticket;
                    round += 1;
                    continue;
                }
            }
        }

        // ---- 情况三：解不出下一段，最后碰碰运气查一下钥匙 ----
        // ---- Case three: nothing to decode, so try the key once just in case ----
        say(talk, "  [轮询] 无后续凭据，尝试查询密钥");
        if let Some(key) = check_key(&ticket, talk, Some(&mut timer)) {
            return Outcome {
                key: Some(key),
                timer,
                stopped_at: "成功 / Success",
            };
        }

        stopped_at = "无后续凭据且无密钥 / No next credential and no key";
        break;
    }

    // 出了循环再查一次，万一刚好好了。
    // One more check after the loop, in case it just became ready.
    if let Some(key) = check_key(&ticket, talk, Some(&mut timer)) {
        return Outcome {
            key: Some(key),
            timer,
            stopped_at: "成功 / Success",
        };
    }

    say(
        talk,
        &format!(
            "\n[失败] 未获取密钥 (原因: {}; 已执行 {}/{} 轮)",
            stopped_at, round, rounds_allowed
        ),
    );

    Outcome {
        key: None,
        timer,
        stopped_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 太快的话能认出来_recognises_being_told_too_fast() {
        let a = json!({ "message": "finishing checkpoints too fast" });
        assert!(says_too_fast(&a));

        let b = json!({ "error": "Please slow down" });
        assert!(says_too_fast(&b));

        let c = json!({ "detail": "TOO MANY requests" });
        assert!(says_too_fast(&c));

        let d = json!({ "message": "invalid payload." });
        assert!(!says_too_fast(&d), "链接废了不是太快 / a dead link is not going too fast");
    }

    #[test]
    fn 余量会自己往下减_margin_shrinks_by_itself() {
        // 从起始值开始，连续顺利应该让余量变小。
        // Starting from the initial value, good runs should shrink the margin.
        MARGIN_MS.store(config::GAP_MARGIN_START.as_millis() as u64, Ordering::Relaxed);
        GOOD_RUNS.store(0, Ordering::Relaxed);

        let before = margin();
        for _ in 0..config::CLEAN_STEPS_TO_RELAX {
            note_good();
        }
        let after = margin();

        assert!(after < before, "顺利了应该减少余量 / good runs should shrink the margin");
    }

    #[test]
    fn 被拒之后余量马上加上去_margin_jumps_after_a_refusal() {
        MARGIN_MS.store(config::GAP_MARGIN_MIN.as_millis() as u64, Ordering::Relaxed);

        let before = margin();
        note_too_fast();
        let after = margin();

        assert!(after > before, "被拒应该马上加余量 / a refusal should raise the margin at once");
    }

    #[test]
    fn 余量不会越界_margin_stays_within_bounds() {
        // 拼命减也不能低于下限。
        // Shrinking hard should still not go below the floor.
        MARGIN_MS.store(config::GAP_MARGIN_MIN.as_millis() as u64, Ordering::Relaxed);
        GOOD_RUNS.store(0, Ordering::Relaxed);
        for _ in 0..100 {
            note_good();
        }
        assert!(
            margin() >= config::GAP_MARGIN_MIN,
            "不该低于下限 / should not go below the floor"
        );

        // 拼命加也不能超过上限。
        // Growing hard should still not exceed the ceiling.
        for _ in 0..100 {
            note_too_fast();
        }
        assert!(
            margin() <= config::GAP_MARGIN_MAX,
            "不该超过上限 / should not exceed the ceiling"
        );
    }
}
