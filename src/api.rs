//! 网页接口
//! The web interface
//!
//! 只有一个地址：`GET /delta?url=<链接>`，绕过之后把钥匙给你。
//!
//! 三件值得说的事：
//!
//! **记钥匙。** 同一条链接绕过成功后，钥匙记 24 小时（跟钥匙自己的有效期一样长）。
//! 再问同一条就直接给，不重新跑。
//!
//! **同一条链接的并发请求合并成一次。** 十个人同时问同一条，只真跑一次，其余九个
//! 等着分同一个结果。不然十个人各跑一次，互相还会撞上那个 5 秒间隔的限制。
//!
//! **接口没有任何验证。** 谁能连上这个端口，谁就能用你的绕过能力。生产环境别直接
//! 对公网开，见 README 的部署那节。
//!
//! One address only: `GET /delta?url=<link>`, which does the bypass and gives you the
//! key.
//!
//! Three things worth saying:
//!
//! **Keys are remembered.** Once a link is bypassed, its key is kept for 24 hours (as
//! long as the key itself lasts). Asking for the same link again gets it straight away
//! without rerunning.
//!
//! **Concurrent requests for one link are merged.** Ten people asking about the same
//! link at once means one real run, with the other nine waiting to share the result.
//! Otherwise all ten would run separately and trip over that five-second gap rule.
//!
//! **The interface has no authentication whatsoever.** Anyone who can reach the port
//! can use your bypass capacity. Do not put it straight on the public internet in
//! production; see the deployment section of the README.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use md5::{Digest, Md5};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::watch;

use crate::auth;
use crate::chain;
use crate::config;
use crate::net;
use crate::pool;

/// 记下来的一个钥匙。
/// One remembered key.
#[derive(Clone)]
struct Remembered {
    key: String,
    /// 什么时候记的（秒）。
    /// When it was remembered, in seconds.
    when: u64,
    /// 当初跑了多久。缓存命中时返回这个数，别让人误以为秒回是真跑出来的。
    /// How long the original run took. Returned on a cache hit, so nobody mistakes an
    /// instant reply for a real run.
    took_secs: f64,
}

/// 一次绕过的结果。
/// The outcome of one bypass.
#[derive(Clone)]
struct Result0 {
    key: Option<String>,
    error: Option<String>,
    took_secs: f64,
}

/// 服务运行时要用的东西。
/// What the service needs while running.
struct Shared {
    /// 记下来的钥匙。
    /// Remembered keys.
    remembered: Mutex<HashMap<String, Remembered>>,
    /// 正在跑的活儿。同一条链接的请求都挂在同一个上面等。
    /// Runs in progress. Requests for one link all wait on the same entry.
    running: Mutex<HashMap<String, watch::Sender<Option<Result0>>>>,
}

/// 现在多少秒了。
/// The time now, in seconds.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 把链接算成一个短名字，用来当记录的钥匙。
/// Turn a link into a short name, used as the lookup key for records.
fn short_name(text: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ---------------------------------------------------------------------------
// 存到文件 / Saving to a file
// ---------------------------------------------------------------------------

/// 从指定文件把记过的钥匙读回来。
///
/// 过期的直接不要。文件坏了或者不存在就当没有，不影响启动。
///
/// 路径是参数而不是写死的，这样测试可以各用各的文件，不必去改进程的当前目录 ——
/// 当前目录是全进程共享的，测试并行跑起来会互相踩。
///
/// Read remembered keys back from the given file.
///
/// Expired ones are simply left out. A missing or damaged file is treated as none, and does
/// not stop startup.
///
/// The path is a parameter rather than hard-coded so that tests can each use their own file
/// instead of changing the process's current directory — that directory is shared across the
/// whole process, and parallel tests tread on each other through it.
fn load_remembered_from(path: &Path) -> HashMap<String, Remembered> {
    let mut out = HashMap::new();

    let Ok(text) = std::fs::read_to_string(path) else {
        return out;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
        return out;
    };
    let Some(table) = parsed.as_object() else {
        return out;
    };

    let now = now_secs();

    for (name, record) in table {
        let Some(fields) = record.as_object() else { continue };

        // 时间戳要按小数读再取整。
        //
        // 线上 Python 版写进去的是带小数的秒（比如 1788249467.7546496），用整数方式
        // 读会直接读不出来 —— 那样接管服务时现有的上千条记录会全部作废，白白重跑。
        //
        // Read the timestamp as a decimal and round it down.
        //
        // The production Python version writes seconds with a fraction (1788249467.7546496
        // for instance), and reading that as a whole number simply fails — which would
        // throw away the thousands of existing records when taking over the service, and
        // rerun them all for nothing.
        let when = fields
            .get("ts")
            .and_then(Value::as_f64)
            .map(|t| t as u64)
            .unwrap_or(0);

        if now.saturating_sub(when) >= config::CACHE_TTL.as_secs() {
            continue;
        }

        let Some(key) = fields.get("key").and_then(Value::as_str) else { continue };

        out.insert(
            name.clone(),
            Remembered {
                key: key.to_string(),
                when,
                took_secs: fields.get("solve_time").and_then(Value::as_f64).unwrap_or(0.0),
            },
        );
    }

    out
}

/// 从默认文件读。服务启动时用这个。
/// Read from the default file. Used at service startup.
fn load_remembered() -> HashMap<String, Remembered> {
    load_remembered_from(Path::new(config::CACHE_FILE))
}

/// 把记过的钥匙写到指定文件。
///
/// 先写临时文件再改名，这样中途断电也不会留下半个坏文件。
///
/// Write remembered keys to the given file.
///
/// It writes a temporary file and renames it, so losing power midway cannot leave half a
/// broken file behind.
fn save_remembered_to(path: &Path, table: &HashMap<String, Remembered>) {
    let mut out = serde_json::Map::new();

    for (name, record) in table {
        out.insert(
            name.clone(),
            json!({
                "key": record.key,
                "ts": record.when,
                "solve_time": record.took_secs,
            }),
        );
    }

    let Ok(text) = serde_json::to_string(&Value::Object(out)) else {
        return;
    };

    let temp = path.with_extension("json.tmp");
    if std::fs::write(&temp, text).is_ok() {
        let _ = std::fs::rename(&temp, path);
    }
}

/// 写到默认文件。求解成功后用这个。
/// Write to the default file. Used after a successful bypass.
fn save_remembered(table: &HashMap<String, Remembered>) {
    save_remembered_to(Path::new(config::CACHE_FILE), table)
}

// ---------------------------------------------------------------------------
// 跑一次绕过 / Running one bypass
// ---------------------------------------------------------------------------

/// 跑一趟。
/// One attempt.
fn run_once(ticket: &str) -> Result0 {
    let started = Instant::now();
    let outcome = chain::run(ticket, false, config::DEFAULT_MAX_ROUNDS);

    let error = if outcome.key.is_some() {
        None
    } else if let Some(why) = &outcome.timer.invalid_reason {
        // 链接本身废了，说清楚，让调用方知道别再试了。
        // The link itself is dead, so say so plainly and let the caller know not to
        // retry.
        Some(format!("链接无效 / Invalid link: {}", why))
    } else {
        Some("绕过失败 / Bypass failed".to_string())
    };

    let took = if !outcome.timer.is_empty() {
        outcome.timer.total()
    } else {
        started.elapsed().as_secs_f64()
    };

    Result0 { key: outcome.key, error, took_secs: took }
}

/// 跑一趟，不行再跑一趟。
///
/// 第一趟失败常常是一时的（网络抖了、验证码没认出来），再来一次多半就好了。
/// 但"链接本身废了"这种不重试 —— 再跑一百次也是废的。
///
/// Try once, and again if it fails.
///
/// A first failure is often momentary (a network wobble, a puzzle not recognised) and
/// a second go usually works. But a dead link is not retried — it would be dead a
/// hundred more times.
fn run_with_retry(ticket: &str) -> Result0 {
    let first = run_once(ticket);

    if first.key.is_some() {
        return first;
    }

    if let Some(why) = &first.error {
        if why.starts_with("链接无效") {
            return first;
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(300));
    run_once(ticket)
}

// ---------------------------------------------------------------------------
// 接口 / The endpoint
// ---------------------------------------------------------------------------

/// 请求参数。
/// Request parameters.
#[derive(Deserialize)]
struct Params {
    url: Option<String>,
}

/// 拼回话。
///
/// 字段跟原来的 Python 版保持一致，方便直接替换：`key` `cached` `error` `made_by`
/// `qq_group` `times`。
///
/// Build the reply.
///
/// The fields match the original Python version so it can be swapped in directly:
/// `key`, `cached`, `error`, `made_by`, `qq_group`, `times`.
fn reply(key: Option<String>, from_memory: bool, error: Option<String>, took: f64) -> Value {
    json!({
        "key": key,
        "cached": from_memory,
        "error": error,
        "made_by": config::MADE_BY,
        "qq_group": config::QQ_GROUP,
        // 这里是真实绕过耗时，不是这次请求的耗时。缓存命中给的是当初那次的数。
        // This is the real bypass time, not this request's time. A cache hit gives the
        // figure from the original run.
        "times": format!("{:.12}s", took),
    })
}

/// 回话统一带上 utf-8 声明。
///
/// 不写清楚的话，有些浏览器会按别的编码去读，中文就成乱码了。
///
/// Every reply states utf-8 explicitly.
///
/// Without saying so, some browsers read it as another encoding and the Chinese comes
/// out as gibberish.
fn as_json(body: Value) -> axum::response::Response {
    let mut out = axum::Json(body).into_response();
    out.headers_mut().insert(
        "content-type",
        "application/json; charset=utf-8".parse().unwrap(),
    );
    out
}

/// 处理一次 `/delta` 请求。
/// Handle one `/delta` request.
async fn delta(
    State(shared): State<Arc<Shared>>,
    Query(params): Query<Params>,
) -> axum::response::Response {
    let started = Instant::now();
    let given = params.url.unwrap_or_default();

    let ticket = auth::extract_ticket(&given);

    // 长度不够的在这里就挡掉，别进求解流程。
    //
    // 求解一趟要 5 秒以上，而这种链接注定失败 —— 早点说清楚比让人等着强。顺带也
    // 省掉一次没意义的自动重试。
    //
    // Anything too short is refused here rather than entering the bypass flow.
    //
    // A bypass takes over 5 seconds and this kind of link is doomed anyway — saying so
    // early beats making the caller wait. It also saves a pointless automatic retry.
    if ticket.len() < auth::MIN_TICKET_LEN {
        return as_json(reply(
            None,
            false,
            Some("链接格式无效 / Malformed link".into()),
            started.elapsed().as_secs_f64(),
        ));
    }

    let name = short_name(&ticket);

    // ---- 先看记过没 ----
    // ---- First see whether we remember it ----
    {
        let table = shared.remembered.lock().unwrap();
        if let Some(record) = table.get(&name) {
            if now_secs().saturating_sub(record.when) < config::CACHE_TTL.as_secs() {
                return as_json(reply(Some(record.key.clone()), true, None, record.took_secs));
            }
        }
    }

    // ---- 同一条链接只跑一次 ----
    // ---- Only one run per link ----
    //
    // 第一个来的负责跑，后来的都挂在同一个通道上等结果。
    // The first to arrive does the running; everyone after waits on the same channel.
    let should_run;
    let mut listener = {
        let mut table = shared.running.lock().unwrap();
        match table.entry(name.clone()) {
            Entry::Vacant(slot) => {
                let (teller, listener) = watch::channel(None);
                slot.insert(teller);
                should_run = true;
                listener
            }
            Entry::Occupied(slot) => {
                should_run = false;
                slot.get().subscribe()
            }
        }
    };

    if should_run {
        let shared = shared.clone();
        let ticket = ticket.clone();
        let name = name.clone();

        tokio::spawn(async move {
            // 绕过过程会阻塞（等 5 秒、算图片），扔到专门跑阻塞活儿的线程上，别占着
            // 处理请求的线程。
            //
            // The bypass blocks (waiting five seconds, working on pictures), so it goes
            // on a thread meant for blocking work rather than tying up a request thread.
            let outcome = tokio::task::spawn_blocking(move || run_with_retry(&ticket))
                .await
                .unwrap_or_else(|_| Result0 {
                    key: None,
                    error: Some("内部执行异常 / Internal execution error".into()),
                    took_secs: 0.0,
                });

            if let Some(key) = &outcome.key {
                let mut table = shared.remembered.lock().unwrap();
                table.insert(
                    name.clone(),
                    Remembered {
                        key: key.clone(),
                        when: now_secs(),
                        took_secs: outcome.took_secs,
                    },
                );

                // 顺手把过期的清掉再落盘。
                //
                // 不清的话，服务一直不重启，过期条目就一直躺在文件里越攒越多 —— 读的
                // 时候会跳过它们，所以不影响结果，但文件白白变大，而且看着像有脏数据。
                //
                // Drop expired entries before writing, while we are here.
                //
                // Without this, a long-running service keeps piling stale entries into the
                // file — reads skip them so results are unaffected, but the file grows for
                // nothing and looks like it holds bad data.
                let now = now_secs();
                let ttl = config::CACHE_TTL.as_secs();
                table.retain(|_, r| now.saturating_sub(r.when) < ttl);

                save_remembered(&table);
            }

            // 顺序很要紧：**先把结果广播出去，再把这条记录去掉。**
            //
            // 反过来的话，通道的发送端一被丢掉，等着的那些人看到的是"通道关了"而不是
            // 结果，然后就都拿到空。
            //
            // The order matters: **broadcast the result first, then remove the entry.**
            //
            // The other way round, dropping the sending end makes everyone waiting see
            // "channel closed" instead of the result, and they all come away empty.
            let teller = shared.running.lock().unwrap().remove(&name);
            if let Some(teller) = teller {
                let _ = teller.send(Some(outcome));
            }
        });
    }

    // ---- 等结果 ----
    // ---- Wait for the result ----
    loop {
        if listener.borrow().is_some() {
            break;
        }
        if listener.changed().await.is_err() {
            break;
        }
    }

    let outcome = listener.borrow().clone().unwrap_or_else(|| Result0 {
        key: None,
        error: Some("未获得结果 / No result returned".into()),
        took_secs: 0.0,
    });

    as_json(reply(outcome.key, false, outcome.error, outcome.took_secs))
}

/// 开始提供服务。
///
/// `ready_puzzles` 是预备池要存多少道，0 就是不备。
///
/// Start serving.
///
/// `ready_puzzles` is how many puzzles to keep ready; 0 turns it off.
pub fn serve(host: &str, port: u16, ready_puzzles: usize) {
    let shared = Arc::new(Shared {
        remembered: Mutex::new(load_remembered()),
        running: Mutex::new(HashMap::new()),
    });

    // 先把连接开好，第一个请求就不用等握手了。
    // Open the connections first, so the very first request skips the handshake.
    std::thread::spawn(net::warm_up_all);

    // 后台开始备题。
    // Start preparing puzzles in the background.
    pool::start(ready_puzzles, (*net::captcha_client()).clone());

    let runtime = tokio::runtime::Runtime::new().expect("初始化运行时 / initialise the runtime");

    runtime.block_on(async move {
        let app = Router::new().route("/delta", get(delta)).with_state(shared);
        let address = format!("{}:{}", host, port);

        let socket = tokio::net::TcpListener::bind(&address)
            .await
            .expect("绑定端口 / bind the port");

        println!("[服务] Delta 绕过器已启动 / Delta bypass service started: http://{}", address);
        println!(
            "[服务] 验证码预备池容量 {} / Captcha pool capacity: {}",
            ready_puzzles, ready_puzzles
        );
        println!(
            "[警告] 接口未启用鉴权，请勿直接暴露至公网 / \
             No authentication enabled; do not expose to the public internet"
        );

        axum::serve(socket, app).await.expect("启动服务 / start serving");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 短名字稳定且不同链接不同_short_name_is_stable_and_distinct() {
        let a = short_name("ticket-one");
        let b = short_name("ticket-one");
        let c = short_name("ticket-two");

        assert_eq!(a, b, "同一条链接应该算出同一个名字 / one link should give one name");
        assert_ne!(a, c, "不同链接应该不同名 / different links should differ");
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn 回话字段齐全_reply_has_all_the_fields() {
        let body = reply(Some("FREE_abc".into()), false, None, 5.5);

        assert_eq!(body["key"], "FREE_abc");
        assert_eq!(body["cached"], false);
        assert!(body["error"].is_null());
        assert_eq!(body["made_by"], config::MADE_BY);
        assert_eq!(body["qq_group"], config::QQ_GROUP);
        assert!(body["times"].as_str().unwrap().ends_with('s'));
    }

    #[test]
    fn 失败时钥匙为空且有原因_failure_has_no_key_but_a_reason() {
        let body = reply(None, false, Some("绕过失败".into()), 1.2);

        assert!(body["key"].is_null(), "没成时钥匙应为空 / no key on failure");
        assert_eq!(body["error"], "绕过失败");
        assert_eq!(body["cached"], false);
    }

    /// 每个测试各用一个临时文件。
    ///
    /// **不能改进程的当前目录。** 当前目录是全进程共享的，测试默认并行跑，一个测试改了
    /// 目录、另一个正好在用，就会互相踩 —— 之前真踩到了：临时目录被先结束的测试删掉，
    /// 后结束的那个要切回原目录时报 NotFound。
    ///
    /// Each test gets its own temporary file.
    ///
    /// **Never change the process's current directory.** It is shared across the whole
    /// process, tests run in parallel by default, and one test changing it while another is
    /// using it means they tread on each other — which actually happened: the temporary
    /// directory was removed by whichever test finished first, and the other failed with
    /// NotFound when trying to change back.
    fn temp_cache() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("临时目录 / temp dir");
        let path = dir.path().join(config::CACHE_FILE);
        (dir, path)
    }

    #[test]
    fn 存了能读回来_saving_then_loading_works() {
        let (_dir, path) = temp_cache();

        let mut table = HashMap::new();
        table.insert(
            "abc".to_string(),
            Remembered { key: "FREE_xyz".into(), when: now_secs(), took_secs: 5.5 },
        );
        save_remembered_to(&path, &table);

        let read_back = load_remembered_from(&path);

        assert_eq!(read_back.len(), 1);
        assert_eq!(read_back.get("abc").unwrap().key, "FREE_xyz");
        assert_eq!(read_back.get("abc").unwrap().took_secs, 5.5);
    }

    #[test]
    fn 能读旧版带小数的时间戳_reads_the_old_fractional_timestamp() {
        // 线上 Python 版写的 ts 带小数。接管服务时必须能读，否则上千条记录白丢。
        // The production Python version writes ts with a fraction. Taking over the
        // service has to read that, or thousands of records are thrown away.
        let (_dir, path) = temp_cache();

        let now = now_secs();
        let python_style = format!(
            r#"{{"abc":{{"key":"FREE_python","ts":{}.7546496,"solve_time":6.812400085}}}}"#,
            now
        );
        std::fs::write(&path, python_style).expect("写文件 / write file");

        let read_back = load_remembered_from(&path);

        assert_eq!(read_back.len(), 1, "带小数的时间戳应该能读出来 / a fractional timestamp should load");
        let record = read_back.get("abc").expect("应该有这条 / the record should be there");
        assert_eq!(record.key, "FREE_python");
        assert_eq!(record.when, now, "小数部分应该被去掉 / the fraction should be dropped");
    }

    #[test]
    fn 过期的读不回来_expired_records_are_left_out() {
        let (_dir, path) = temp_cache();

        let mut table = HashMap::new();
        table.insert(
            "old".to_string(),
            Remembered {
                key: "FREE_old".into(),
                // 两天前，早过期了。
                // Two days ago, long expired.
                when: now_secs() - 48 * 3600,
                took_secs: 5.0,
            },
        );
        save_remembered_to(&path, &table);

        let read_back = load_remembered_from(&path);

        assert!(read_back.is_empty(), "过期的不该读回来 / expired ones should not come back");
    }

    #[test]
    fn 文件不在或坏了都当空_missing_or_broken_file_reads_as_empty() {
        let (_dir, path) = temp_cache();

        // 文件根本不存在。
        // The file does not exist at all.
        assert!(load_remembered_from(&path).is_empty());

        // 不是 JSON。
        // Not JSON.
        std::fs::write(&path, "这不是 JSON / not JSON").expect("写文件 / write file");
        assert!(load_remembered_from(&path).is_empty());

        // 是 JSON 但不是对象。
        // JSON, but not an object.
        std::fs::write(&path, "[1, 2, 3]").expect("写文件 / write file");
        assert!(load_remembered_from(&path).is_empty());

        // 对象里的记录缺字段。
        // A record inside is missing fields.
        std::fs::write(&path, r#"{"abc":{"ts":123}}"#).expect("写文件 / write file");
        assert!(load_remembered_from(&path).is_empty());
    }

    /// 落盘时会把过期的清掉，文件里不该越攒越多。
    ///
    /// 之前只在启动时淘汰，服务长期不重启就会一直积压。验收里逮到过：文件里躺着一条
    /// 超期 79 秒的。
    ///
    /// Expired entries are dropped when writing, so the file does not keep growing.
    ///
    /// Eviction used to happen only at startup, so a long-running service piled them up.
    /// The check caught one sitting 79 seconds past its time.
    #[test]
    fn 落盘会清掉过期的_writing_drops_expired_entries() {
        let (_dir, path) = temp_cache();
        let now = now_secs();
        let ttl = config::CACHE_TTL.as_secs();

        let mut table = HashMap::new();
        // 一条新的，一条刚好超期一点的。
        // One fresh, one just past its time.
        table.insert(
            "fresh".to_string(),
            Remembered { key: "FREE_fresh".into(), when: now, took_secs: 5.5 },
        );
        table.insert(
            "stale".to_string(),
            Remembered { key: "FREE_stale".into(), when: now - ttl - 79, took_secs: 5.5 },
        );

        // 模拟落盘前的清理。
        // Mimic the tidy-up that happens before writing.
        table.retain(|_, r| now.saturating_sub(r.when) < ttl);
        save_remembered_to(&path, &table);

        // 直接看文件内容，确认过期那条根本没写进去。
        // Read the file itself, to confirm the expired one never made it in.
        let raw = std::fs::read_to_string(&path).expect("读文件 / read file");
        assert!(raw.contains("FREE_fresh"), "新的应该在 / the fresh one should be there");
        assert!(
            !raw.contains("FREE_stale"),
            "过期的不该写进文件 / the expired one should not be written"
        );

        let read_back = load_remembered_from(&path);
        assert_eq!(read_back.len(), 1);
    }
}
