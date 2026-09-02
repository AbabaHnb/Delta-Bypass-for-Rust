//! 提前备好验证码
//! Prepare captcha puzzles in advance
//!
//! ## 为什么能提前备
//!
//! 验证码跟具体链接**没关系**。这是实测过的：先拿一道题算出答案换成通行凭证，
//! 然后再新建一条链接，用那个凭证照样能提交成功。所以出题的服务是"公用"的，不是
//! 服务器专门为某条链接下发的。
//!
//! 既然这样，那道题的耗时（拿题 400 毫秒 + 下载 45KB 图片 700 毫秒 + 算 100 毫秒）
//! 就可以提前在后台花掉，用户来的时候只剩最后一步。
//!
//! ## 备的是什么，不备什么
//!
//! **备**：题目编号 + 算好的坐标。
//! **不备**：通行凭证。凭证是用户来的那一刻才去换的，换完立刻用掉。绝不缓存、绝不
//! 复用。
//!
//! ## 速度是这里最要紧的事
//!
//! 存 30 道题、每道活 30 秒，意味着**每秒必须补一道**，不管有没有人用。一道题要
//! 两个请求（拿题 + 下图），也就是每秒两个请求，永远如此。
//!
//! 实测：每秒一道可以一直跑（连续 70 次全对）。但**一拥而上就会被限流**，而且被限
//! 流之后连"当场做题"这条路也一起断了 —— 整个绕过器就瘫了。所以：
//!
//! - 全局只有一个"发牌口"，所有后台线程都得排队领牌才能做题
//! - 同时最多做两道
//! - 被拒一次，**所有**线程一起歇（这点最关键：各自重试会把限流一直续着）
//!
//! ## Why this can be done in advance
//!
//! Picture puzzles have **nothing to do with a particular link**. This was tested:
//! take a puzzle, work out the answer, swap it for a pass token, *then* create a
//! brand-new link — and that token still submits fine. So the puzzle service is
//! shared, not something the server issues per link.
//!
//! Given that, a puzzle's cost (400ms to fetch it, 700ms to download the 45KB
//! picture, 100ms to work it out) can be spent in the background beforehand, leaving
//! only the last step for when someone actually asks.
//!
//! ## What is kept ready, and what is not
//!
//! **Kept**: the puzzle number and the worked-out position.
//! **Not kept**: the pass token. The token is swapped for at the moment someone asks
//! and used immediately. Never cached, never reused.
//!
//! ## Speed is the thing that matters most here
//!
//! Keeping 30 puzzles that each live 30 seconds means **one must be topped up every
//! second**, whether or not anyone is using them. One puzzle takes two requests
//! (fetch plus picture download), so that is two requests per second, forever.
//!
//! Measured: one per second runs indefinitely (70 in a row, all fine). But **rushing
//! several at once gets us rate-limited**, and once that happens even the "do one on
//! the spot" path breaks too — the whole bypass stops working. Hence:
//!
//! - one global ticket window, and every background thread must queue for a ticket
//! - at most two puzzles in progress at once
//! - one refusal rests **every** thread (this is the crucial part: retrying
//!   separately keeps the rate limit alive indefinitely)

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::config;
use crate::solver;

/// 一道备好的题：算好答案，但还没换凭证。
/// A puzzle that is ready: the answer is worked out, but no token has been swapped
/// for yet.
#[derive(Clone)]
pub struct Ready {
    /// 题目编号。
    /// The puzzle number.
    pub id: String,
    /// 算出来该点的位置。
    /// The position we worked out to click.
    pub x: f64,
    pub y: f64,
    /// 题的种类。
    /// The kind of puzzle.
    pub kind: String,
    /// 用了哪套办法算出来的，看日志时有用。
    /// Which method worked it out, handy when reading logs.
    pub method: &'static str,
    /// 算这道题花了多少毫秒。
    /// How many milliseconds it took to work out.
    pub took_ms: f64,
    /// 什么时候备好的。
    /// When it became ready.
    pub made_at: Instant,
}

impl Ready {
    /// 还没过期。
    /// Not expired yet.
    fn alive(&self) -> bool {
        self.made_at.elapsed() < config::POOL_MAX_AGE
    }

    /// 不光没过期，还剩够时间去换凭证。
    ///
    /// 换凭证要一趟来回（约 200 毫秒），要是发出去的时候只剩 100 毫秒，换回来就过期了。
    ///
    /// Not only unexpired, but with enough time left to swap for a token.
    ///
    /// The swap takes a round trip (about 200ms), so handing out one with 100ms left
    /// means it expires before the swap comes back.
    fn usable(&self) -> bool {
        self.made_at.elapsed() + config::POOL_USE_MARGIN < config::POOL_MAX_AGE
    }
}

/// 做一道题的结果。
/// The outcome of trying to prepare one puzzle.
enum Outcome {
    /// 做好了。
    /// Done.
    Done(Ready),
    /// 服务器不让做了（限流之类）。**这种必须歇，不能立刻重试。**
    /// The server said no (rate limit or similar). **This must be rested, not
    /// retried straight away.**
    Refused,
    /// 网络抖了一下，或者这张图算不出来。歇一小会儿就行。
    /// A network hiccup, or a picture we could not work out. A short rest is enough.
    Hiccup,
}

/// 预备池。
/// The puzzle store.
struct Store {
    /// 备好的题，先进先出。
    /// Puzzles that are ready, first in first out.
    queue: Mutex<VecDeque<Ready>>,
    /// 用来叫醒等着的线程。
    /// Used to wake up waiting threads.
    bell: Condvar,
    /// 想存多少道。
    /// How many we aim to keep.
    want: AtomicUsize,
    /// 后台线程起来了没，防止重复启动。
    /// Whether the background threads are running, so they are not started twice.
    started: AtomicBool,

    /// 一共做了多少道。
    /// How many were prepared in total.
    made: AtomicUsize,
    /// 一共发出去多少道。
    /// How many were handed out in total.
    handed_out: AtomicUsize,
    /// 一共过期扔掉多少道。
    /// How many expired and were thrown away.
    expired: AtomicUsize,
    /// 被服务器拒了多少次。
    /// How many times the server refused us.
    refused: AtomicUsize,

    /// 正在做的有几道。
    /// How many are being prepared right now.
    in_progress: AtomicUsize,

    /// 下一张牌什么时候能发（相对 `start_time` 的纳秒数）。
    /// When the next ticket can be issued, in nanoseconds since `start_time`.
    next_ticket_ns: Mutex<u128>,
    /// 计时起点。
    /// The point we measure from.
    start_time: Mutex<Option<Instant>>,

    /// 当前罚站时长（毫秒）。被拒就加倍，顺利就减半。
    /// The current penalty rest in milliseconds. Doubles on a refusal, halves when
    /// things go well.
    penalty_ms: AtomicU64,
}

/// 全程就一个池子。
/// One store for the whole program.
fn store() -> &'static Arc<Store> {
    static S: std::sync::OnceLock<Arc<Store>> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        Arc::new(Store {
            queue: Mutex::new(VecDeque::new()),
            bell: Condvar::new(),
            want: AtomicUsize::new(0),
            started: AtomicBool::new(false),
            made: AtomicUsize::new(0),
            handed_out: AtomicUsize::new(0),
            expired: AtomicUsize::new(0),
            refused: AtomicUsize::new(0),
            in_progress: AtomicUsize::new(0),
            next_ticket_ns: Mutex::new(0),
            start_time: Mutex::new(None),
            penalty_ms: AtomicU64::new(0),
        })
    })
}

impl Store {
    /// 从计时起点到现在多少纳秒。
    /// How many nanoseconds since we started measuring.
    fn since_start_ns(&self) -> u128 {
        let mut start = self.start_time.lock().unwrap();
        let point = *start.get_or_insert_with(Instant::now);
        point.elapsed().as_nanos()
    }

    /// 两道题之间该隔多久。
    ///
    /// 算法很朴素：要存 N 道、每道活 T 秒，那就每 T/N 秒补一道，池子自然就稳在 N 道
    /// 左右，不用额外攒富余。
    ///
    /// 算出来再跟"最快速度"取大的 —— 想存太多的话，宁可存不满也不能超速被限流。
    /// 最后加上当前罚站时长。
    ///
    /// How long to leave between puzzles.
    ///
    /// The sum is plain: to keep N puzzles that each live T seconds, top one up every
    /// T/N seconds, and the store settles around N by itself with no spare needed.
    ///
    /// Then take whichever is longer, that or the fastest allowed rate — if the target
    /// is too high we would rather fall short than speed up and get rate-limited.
    /// Finally, add the current penalty rest.
    fn gap_between(&self) -> Duration {
        let want = self.want.load(Ordering::Relaxed).max(1) as u32;
        let window = config::POOL_MAX_AGE.saturating_sub(config::POOL_USE_MARGIN);
        let even_spacing = window / want;
        let penalty = Duration::from_millis(self.penalty_ms.load(Ordering::Relaxed));

        even_spacing.max(config::POOL_MIN_SLOT_INTERVAL) + penalty
    }

    /// 领一张牌。领到了才能做题；领不到就返回"还要等多久"。
    ///
    /// 注意下面那行是从**现在**开始算下一张牌的时间，不是从上一张的时间往后加。
    /// 这很关键：要是往后累加，闲了一分钟就会攒下一大堆牌，一下全发出去，正好触发
    /// 限流。
    ///
    /// Take a ticket. Only with one can a puzzle be prepared; without, it reports how
    /// much longer to wait.
    ///
    /// Note that the next ticket time is worked out from **now**, not by adding onto
    /// the previous one. This matters: adding on would bank up a pile of tickets over
    /// an idle minute and release them all at once, which is exactly what triggers the
    /// rate limit.
    fn take_ticket(&self) -> Result<(), Duration> {
        let gap = self.gap_between();
        let now = self.since_start_ns();
        let mut next = self.next_ticket_ns.lock().unwrap();

        if now >= *next {
            *next = now + gap.as_nanos();
            Ok(())
        } else {
            Err(Duration::from_nanos((*next - now) as u64))
        }
    }

    /// 被拒了：罚站时间加倍（第一次用起始值）。
    /// Refused: double the penalty rest, starting from the initial value the first
    /// time.
    fn note_refused(&self) {
        self.refused.fetch_add(1, Ordering::Relaxed);

        let now = self.penalty_ms.load(Ordering::Relaxed);
        let longer = if now == 0 {
            config::POOL_BACKOFF_START.as_millis() as u64
        } else {
            (now * 2).min(config::POOL_BACKOFF_MAX.as_millis() as u64)
        };

        self.penalty_ms.store(longer, Ordering::Relaxed);
    }

    /// 顺利了：罚站时间减半，慢慢恢复正常速度。
    /// Went well: halve the penalty rest, easing back to normal speed.
    fn note_fine(&self) {
        let now = self.penalty_ms.load(Ordering::Relaxed);
        if now > 0 {
            self.penalty_ms.store(now / 2, Ordering::Relaxed);
        }
    }

    /// 清掉过期的，数一数还有多少能用的。
    /// Clear out the expired ones and count how many are still usable.
    fn tidy_and_count(&self) -> usize {
        let mut queue = self.queue.lock().unwrap();

        let before = queue.len();
        queue.retain(|r| r.alive());
        let thrown = before - queue.len();
        if thrown > 0 {
            self.expired.fetch_add(thrown, Ordering::Relaxed);
        }

        queue.iter().filter(|r| r.usable()).count()
    }
}

/// 做一道题：拿题、下图、算答案。不换凭证。
/// Prepare one puzzle: fetch it, download the picture, work out the answer. No token
/// swap.
fn prepare_one(client: &reqwest::blocking::Client) -> Outcome {
    // 第一步：拿一道题。
    // Step one: fetch a puzzle.
    let reply = match client
        .get(format!("{}/challenge", config::CAPTCHA_API))
        .timeout(config::IMAGE_TIMEOUT)
        .send()
    {
        Ok(r) => r,
        Err(_) => return Outcome::Hiccup,
    };

    // 被限流不能当成"抖了一下"。当成抖动就会立刻重试，那是在给限流续命。
    // A rate limit must not be treated as a hiccup. Treating it as one means retrying
    // straight away, which keeps the limit alive.
    if !reply.status().is_success() {
        return Outcome::Refused;
    }

    let puzzle: serde_json::Value = match reply.json() {
        Ok(v) => v,
        Err(_) => return Outcome::Hiccup,
    };

    let (id, kind, picture_path) = match (
        puzzle.get("challenge_id").and_then(|v| v.as_str()),
        puzzle.get("type").and_then(|v| v.as_str()),
        puzzle.get("image").and_then(|v| v.as_str()),
    ) {
        (Some(a), Some(b), Some(c)) => (a.to_string(), b.to_string(), c.to_string()),
        _ => return Outcome::Hiccup,
    };

    // 第二步：下载图片。
    // Step two: download the picture.
    let picture_reply = match client
        .get(format!("{}{}", config::CAPTCHA_HOST, picture_path))
        .header("Referer", format!("{}/", config::CAPTCHA_HOST))
        .timeout(config::IMAGE_TIMEOUT)
        .send()
    {
        Ok(r) => r,
        Err(_) => return Outcome::Hiccup,
    };

    if !picture_reply.status().is_success() {
        return Outcome::Refused;
    }

    let picture = match picture_reply.bytes() {
        Ok(b) => b,
        Err(_) => return Outcome::Hiccup,
    };

    // 第三步：看图选点。
    // Step three: look at the picture and pick a spot.
    let started = Instant::now();
    let (choice, method) = solver::solve(&picture, &kind);
    let took_ms = started.elapsed().as_secs_f64() * 1000.0;

    match choice {
        Some((x, y)) => Outcome::Done(Ready {
            id,
            x,
            y,
            kind,
            method,
            took_ms,
            made_at: Instant::now(),
        }),
        // 这张实在选不出来，扔了换下一张。
        // Nothing could be chosen from this one, so drop it and move on.
        None => Outcome::Hiccup,
    }
}

/// 开始在后台备题。
///
/// 重复调用只会调整目标数量，不会重复起线程。`want` 传 0 就是不备。
///
/// Start preparing puzzles in the background.
///
/// Calling it again only adjusts the target; it does not start more threads. Pass 0
/// for `want` to turn it off.
pub fn start(want: usize, client: reqwest::blocking::Client) {
    let s = store();
    s.want.store(want, Ordering::Relaxed);

    if want == 0 || s.started.swap(true, Ordering::SeqCst) {
        return;
    }

    for _ in 0..config::POOL_WORKERS {
        let s = s.clone();
        let client = client.clone();

        std::thread::spawn(move || loop {
            let want = s.want.load(Ordering::Relaxed);
            let usable = s.tidy_and_count();

            // 够了就歇着，等被叫醒或者到点再看。
            // Enough already, so rest until woken or the next check.
            if usable >= want {
                let queue = s.queue.lock().unwrap();
                let _ = s.bell.wait_timeout(queue, Duration::from_millis(250));
                continue;
            }

            // 同时做的太多了，等一下。
            // Too many in progress at once, so wait.
            if s.in_progress.load(Ordering::Relaxed) >= config::POOL_MAX_INFLIGHT {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }

            // 领牌。领不到就等着 —— 包括冷启动的时候。
            //
            // 有人会想"刚启动池子是空的，先冲一波填满"。别这么干：那正是触发限流的
            // 操作。而且池子填一半也照样能服务，冲这一波没有实际收益。
            //
            // Take a ticket. Without one, wait — including at cold start.
            //
            // Someone will want to say "the store is empty at startup, so rush to fill
            // it". Do not: that is exactly what triggers the rate limit. A half-filled
            // store serves requests perfectly well, so the rush buys nothing.
            if let Err(wait) = s.take_ticket() {
                std::thread::sleep(wait.min(Duration::from_millis(500)));
                continue;
            }

            s.in_progress.fetch_add(1, Ordering::Relaxed);
            let outcome = prepare_one(&client);
            s.in_progress.fetch_sub(1, Ordering::Relaxed);

            match outcome {
                Outcome::Done(ready) => {
                    s.note_fine();

                    let mut queue = s.queue.lock().unwrap();
                    // 再数一遍：刚才做题这一会儿，别的线程可能已经补够了。
                    // Count again: while this one was working, another thread may
                    // already have topped things up.
                    let now_usable = queue.iter().filter(|r| r.usable()).count();
                    if now_usable < s.want.load(Ordering::Relaxed) {
                        queue.push_back(ready);
                        s.made.fetch_add(1, Ordering::Relaxed);
                        s.bell.notify_one();
                    }
                }
                Outcome::Refused => {
                    s.note_refused();
                    let rest = Duration::from_millis(s.penalty_ms.load(Ordering::Relaxed));
                    std::thread::sleep(rest.min(config::POOL_BACKOFF_MAX));
                }
                Outcome::Hiccup => std::thread::sleep(Duration::from_millis(500)),
            }
        });
    }
}

/// 领一道备好的题。
///
/// 池子暂时空了会稍等一下；还是没有就返回空，调用方自己当场做一道。
///
/// 先进先出：先备好的先发，别让老的一直放到过期。
///
/// Take one ready-made puzzle.
///
/// If the store is momentarily empty it waits briefly; if still nothing it returns
/// nothing, and the caller does one on the spot instead.
///
/// First in first out: the oldest goes out first, so none sits around until it
/// expires.
pub fn take() -> Option<Ready> {
    let s = store();

    if s.want.load(Ordering::Relaxed) == 0 {
        return None;
    }

    let mut queue = s.queue.lock().unwrap();

    loop {
        while let Some(ready) = queue.pop_front() {
            if ready.usable() {
                s.handed_out.fetch_add(1, Ordering::Relaxed);
                // 顺手叫醒一个线程去补。
                // Wake a thread to top up while we are here.
                s.bell.notify_one();
                return Some(ready);
            }
            // 剩余时间不够了，扔掉再看下一个。
            // Not enough time left, so throw it away and look at the next.
            s.expired.fetch_add(1, Ordering::Relaxed);
        }

        let (waited, timed_out) = s
            .bell
            .wait_timeout(queue, config::POOL_TAKE_TIMEOUT)
            .unwrap();
        queue = waited;

        if timed_out.timed_out() {
            return None;
        }
    }
}

/// 用一道备好的题换通行凭证。
///
/// 换到就立刻返回给调用方用掉。这里不存、不缓存。
///
/// Swap a ready-made puzzle for a pass token.
///
/// Once swapped it goes straight back to the caller to be used. Nothing is stored or
/// cached here.
pub fn swap_for_token(
    client: &reqwest::blocking::Client,
    ready: &Ready,
) -> Result<(String, f64), String> {
    let started = Instant::now();

    let reply: serde_json::Value = client
        .post(format!("{}/answer", config::CAPTCHA_API))
        .json(&json!({ "challenge_id": ready.id, "x": ready.x, "y": ready.y }))
        .timeout(config::IMAGE_TIMEOUT)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    let took_ms = started.elapsed().as_secs_f64() * 1000.0;

    if reply.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return Err("服务端校验未通过 / Server rejected the answer".into());
    }

    let token = reply
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    if token.is_empty() {
        return Err("校验通过但未返回令牌 / Verified but no token returned".into());
    }

    Ok((token, took_ms))
}

// ---------------------------------------------------------------------------
// 看池子状况 / Checking on the store
// ---------------------------------------------------------------------------

/// 池子当时的情况，给日志和诊断用。
/// A snapshot of the store, for logs and diagnostics.
pub struct Snapshot {
    /// 现在有几道能用。
    /// How many are usable right now.
    pub usable: usize,
    /// 目标是几道。
    /// How many we are aiming for.
    pub want: usize,
    /// 最老那道放了多少秒。
    /// How many seconds the oldest has been sitting.
    pub oldest_secs: f64,
    /// 最新那道放了多少秒。
    /// How many seconds the newest has been sitting.
    pub newest_secs: f64,
    /// 一共做了多少道。
    /// How many were prepared in total.
    pub made: usize,
    /// 一共发出去多少道。
    /// How many were handed out in total.
    pub handed_out: usize,
    /// 一共扔了多少道。
    /// How many were thrown away in total.
    pub expired: usize,
    /// 被拒了多少次。
    /// How many times we were refused.
    pub refused: usize,
    /// 当前罚站多少毫秒。0 就是一切正常。
    /// The current penalty rest in milliseconds. 0 means all is well.
    pub penalty_ms: u64,
}

/// 看一眼池子现在什么情况。
/// Take a look at how the store is doing.
pub fn snapshot() -> Snapshot {
    let s = store();
    let queue = s.queue.lock().unwrap();

    let ages: Vec<f64> = queue
        .iter()
        .filter(|r| r.alive())
        .map(|r| r.made_at.elapsed().as_secs_f64())
        .collect();

    let (oldest, newest) = if ages.is_empty() {
        (0.0, 0.0)
    } else {
        (
            ages.iter().cloned().fold(0.0f64, f64::max),
            ages.iter().cloned().fold(f64::INFINITY, f64::min),
        )
    };

    Snapshot {
        usable: queue.iter().filter(|r| r.usable()).count(),
        want: s.want.load(Ordering::Relaxed),
        oldest_secs: oldest,
        newest_secs: newest,
        made: s.made.load(Ordering::Relaxed),
        handed_out: s.handed_out.load(Ordering::Relaxed),
        expired: s.expired.load(Ordering::Relaxed),
        refused: s.refused.load(Ordering::Relaxed),
        penalty_ms: s.penalty_ms.load(Ordering::Relaxed),
    }
}

/// 等到至少有一道备好的题，或者等够 `how_long` 就放弃。
/// Wait until at least one puzzle is ready, or give up after `how_long`.
pub fn wait_until_ready(how_long: Duration) -> bool {
    let s = store();

    if s.want.load(Ordering::Relaxed) == 0 {
        return false;
    }

    let deadline = Instant::now() + how_long;

    loop {
        if s.queue.lock().unwrap().iter().any(|r| r.usable()) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 叫后台线程现在就看一眼要不要补。
///
/// 用在"接下来肯定要闲一会儿"的时候，比如那 5 秒的强制等待期间。
///
/// Nudge the background threads to check whether topping up is needed now.
///
/// Used when we know there is idle time coming, such as during the enforced
/// five-second wait.
pub fn nudge() {
    let s = store();
    if s.want.load(Ordering::Relaxed) > 0 {
        s.bell.notify_all();
    }
}
