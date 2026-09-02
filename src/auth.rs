//! 跟登录服务器说话
//! Talk to the login server
//!
//! 这里管四件事：
//! 1. 从各种形式的输入里把"通行串"抠出来（网址、纯字符串、文件）
//! 2. 拼出上游要的那两段加密内容
//! 3. 提交一次，往前推进一关
//! 4. 问服务器这个链接的情况、钥匙好了没
//!
//! This file handles four things:
//! 1. Pull the "pass string" out of whatever was given (a web address, a plain
//!    string, or a file)
//! 2. Build the two encrypted pieces the far end wants
//! 3. Submit once, to move one checkpoint forward
//! 4. Ask the server about the link, and whether the key is ready

use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde_json::{json, Value};

use crate::config;
use crate::crypto;
use crate::useragent;

// ---------------------------------------------------------------------------
// 网络连接 / Network connection
// ---------------------------------------------------------------------------

/// 全程共用的连接。
///
/// 两点值得说：
///
/// 一是不自动跟随跳转。上游偶尔会把请求甩到另一个页面去，那通常是它当时忙，
/// 原地重试一下就好。要是自动跟过去，就会拿到一个跟本来目的无关的页面，还以为
/// 成功了。
///
/// 二是连接留着不关。留着的连接发一次请求大约 0.17 秒，重新建要多花一倍时间在
/// 握手上。
///
/// One connection shared throughout.
///
/// Two things worth noting.
///
/// First, it does not follow redirects. The far end sometimes bounces a request
/// to another page, which usually just means it was busy; retrying on the spot
/// works. Following along would land us on an unrelated page while thinking we
/// had succeeded.
///
/// Second, connections are kept open. A request over an open connection takes
/// about 0.17 seconds; opening a new one spends roughly as long again on the
/// handshake.
fn client() -> &'static reqwest::blocking::Client {
    static C: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(config::CONNECT_TIMEOUT)
            .timeout(config::REQUEST_TIMEOUT)
            .pool_idle_timeout(config::POOL_IDLE_TIMEOUT)
            .pool_max_idle_per_host(64)
            .tcp_keepalive(Duration::from_secs(60))
            .tcp_nodelay(true)
            .build()
            .expect("初始化认证客户端 / initialise the auth client")
    })
}

/// 先开一条连接放着，这样第一个真请求不用等握手。
/// Open a connection ahead of time, so the first real request skips the handshake.
pub fn warm_up() {
    let _ = client()
        .get(format!("{}/session/status?ticket=warmup", config::AUTH_API))
        .timeout(Duration::from_secs(5))
        .send()
        .and_then(|r| r.bytes());
}

/// 现在的毫秒时间戳。
/// Current time in milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 抠出通行串 / Pulling out the pass string
// ---------------------------------------------------------------------------

/// 从输入里把通行串找出来。
///
/// 支持三种给法：完整网址（取 `d=` 后面那段）、直接一串、或者一个文件路径。
///
/// Find the pass string in whatever was given.
///
/// Three forms work: a full web address (take what follows `d=`), the string on
/// its own, or a path to a file.
pub fn extract_ticket(input: &str) -> String {
    let text = input.trim();

    if text.starts_with("http") {
        if let Ok(parsed) = url::Url::parse(text) {
            if let Some(found) = parsed
                .query_pairs()
                .find(|(k, _)| k == "d")
                .map(|(_, v)| v.into_owned())
            {
                return found;
            }
        }
        // 网址里没有 d=，原样返回，让后面的流程去报错。
        // No d= in the address; hand it back as-is and let a later step complain.
        return text.to_string();
    }

    // 看着像路径就当文件读，读到内容再递归处理一次。
    // If it looks like a path, read it as a file and process the contents again.
    if text.ends_with(".txt") || text.contains('/') || text.contains('\\') {
        if let Ok(contents) = std::fs::read_to_string(text) {
            let inner = contents.trim().to_string();
            if !inner.is_empty() {
                return extract_ticket(&inner);
            }
        }
    }

    text.to_string()
}

/// 从广告页链接里解出回调地址。
///
/// 地址藏在 `r=` 参数里，用 base64 编过。上游用的是标准 base64 带补位，但也见过
/// 换过字符的写法，所以先把 `-` `_` 换回 `+` `/`，再补齐长度，两种都能解。
///
/// Decode the callback address out of an advert page link.
///
/// The address sits in the `r=` parameter, base64 encoded. The far end uses
/// standard base64 with padding, but the swapped-character form shows up too, so
/// we turn `-` and `_` back into `+` and `/` and top up the length — both then
/// decode.
pub fn decode_callback(advert_url: &str) -> Option<String> {
    let parsed = url::Url::parse(advert_url).ok()?;
    let raw = parsed.query_pairs().find(|(k, _)| k == "r")?.1.into_owned();
    if raw.is_empty() {
        return None;
    }

    let mut fixed = raw.replace('-', "+").replace('_', "/");
    let short_by = (4 - fixed.len() % 4) % 4;
    fixed.push_str(&"=".repeat(short_by));

    let bytes = base64::engine::general_purpose::STANDARD.decode(&fixed).ok()?;
    let text = String::from_utf8_lossy(&bytes).to_string();

    // 解出来得是个网址才算数。
    // It only counts if what came out is a web address.
    if text.starts_with("http") {
        Some(text)
    } else {
        None
    }
}

/// 从回调地址里取下一段通行串。
/// Take the next pass string out of a callback address.
pub fn ticket_from_callback(callback_url: &str) -> Option<String> {
    let parsed = url::Url::parse(callback_url).ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == "d")
        .map(|(_, v)| v.into_owned())
}

// ---------------------------------------------------------------------------
// 拼加密内容 / Building the encrypted pieces
// ---------------------------------------------------------------------------

/// 提交时要带的两段加密内容。
/// The two encrypted pieces sent with a submission.
pub struct Payload {
    /// 装浏览器信息的那段。
    /// The piece carrying browser details.
    pub meta: String,
    /// 装动作记录的那段。
    /// The piece carrying the action record.
    pub stream: String,
}

/// 拼出这两段内容。
///
/// 锁的钥匙就是通行串自己切出来的：前 16 个字符做钥匙、第 17 到 32 个做起始值，
/// 这是第一段；第二段整体往后挪一个字符。上游就这么定的，位置错一个都过不了。
///
/// 传进来的浏览器说明必须跟请求头里写的那个一样。头里说是 iPhone、内容里写另
/// 一款，一眼就看出是编的。
///
/// Build the two pieces.
///
/// The lock keys come out of the pass string itself: characters 1 to 16 as the
/// key and 17 to 32 as the starting value for the first piece; the second piece
/// shifts one character along. That is how the far end defined it — off by one
/// character and it fails.
///
/// The browser note passed in must match the one in the request header. Saying
/// iPhone in the header and something else inside is an obvious giveaway.
pub fn build_payload(ticket: &str, when_ms: u64, agent: &str, screen: &str) -> Payload {
    let bytes = ticket.as_bytes();
    debug_assert!(
        bytes.len() >= 33,
        "凭据长度不足，无法派生密钥 / Credential too short to derive keys from"
    );

    let browser_part = format!(
        r#"{{"browserInfo":[{{"screen":"{}","ua":"{}","time":{}}}]}}"#,
        escape(screen),
        escape(agent),
        when_ms
    );
    let action_part = format!(r#"{{"events":[{{"event":1,"data":{{"time":{}}}}}]}}"#, when_ms);

    Payload {
        meta: crypto::to_hex(&crypto::aes_ctr(
            &bytes[0..16],
            &bytes[16..32],
            browser_part.as_bytes(),
        )),
        stream: crypto::to_hex(&crypto::aes_ctr(
            &bytes[1..17],
            &bytes[17..33],
            action_part.as_bytes(),
        )),
    }
}

/// 把引号之类的字符转义，免得拼出来的内容格式坏掉。
/// Escape quotes and the like, so the text we build does not come out malformed.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 提交和查询 / Submitting and asking
// ---------------------------------------------------------------------------

/// 提交一次，往前推进一关。
///
/// 遇到网络抖动会当场重试几次。返回的就是服务器给的原始内容。
///
/// Submit once, to move one checkpoint forward.
///
/// Network hiccups get retried on the spot a few times. What comes back is the
/// server's reply as-is.
pub fn submit(ticket: &str, token: &str, service: i64) -> Value {
    // 浏览器说明取一次，请求头和加密内容共用同一个。
    // Take the browser note once and use it for both the header and the
    // encrypted content.
    let ua = useragent::next();
    let payload = build_payload(ticket, now_ms(), ua.agent, ua.screen);

    let address = format!(
        "{}/session/step?ticket={}&service={}",
        config::AUTH_API,
        urlencoding::encode(ticket),
        service
    );
    let body = json!({
        "captcha": token,
        "meta": payload.meta,
        "stream": payload.stream,
        "resolved": true
    })
    .to_string();

    let mut last_problem: Option<String> = None;

    for attempt in 0..=config::STEP_HTTP_RETRIES {
        let sent = client()
            .put(&address)
            .body(body.clone())
            .header("User-Agent", ua.agent)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/plain, */*")
            .send();

        match sent {
            Ok(reply) if reply.status() == reqwest::StatusCode::OK => match reply.json::<Value>() {
                Ok(parsed) => return parsed,
                Err(_) => last_problem = Some("响应解析失败 / Failed to parse response".into()),
            },
            Ok(reply) => {
                last_problem = Some(format!("状态 {} / status {}", reply.status().as_u16(), reply.status().as_u16()));
            }
            Err(e) => last_problem = Some(e.to_string()),
        }

        if attempt < config::STEP_HTTP_RETRIES {
            std::thread::sleep(config::STEP_RETRY_SLEEP);
        }
    }

    json!({
        "success": false,
        "error": last_problem.unwrap_or_else(|| "提交失败 / Submission failed".into())
    })
}

/// 发一个查询请求，把回话读成结构化内容。
///
/// 失败时返回的内容里会带 `transient: true`，意思是"这次是网络/服务器一时的问
/// 题，不代表链接坏了"。这个区分很重要：链接真坏了要立刻停，一时的问题该重试。
///
/// Send a query and read the reply into structured content.
///
/// On failure the returned content carries `transient: true`, meaning "this was a
/// momentary network or server problem, not a broken link". That distinction
/// matters: a genuinely broken link should stop us immediately, a momentary
/// problem should be retried.
pub fn query(path_and_params: &str, retries: usize, rest: Duration) -> Value {
    let mut last_problem: Option<String> = None;

    for attempt in 0..=retries {
        let sent = client()
            .get(format!("{}/{}", config::AUTH_API, path_and_params))
            .header("User-Agent", useragent::next().agent)
            .header("Accept", "application/json")
            .send();

        match sent {
            Ok(reply) if reply.status() == reqwest::StatusCode::OK => match reply.json::<Value>() {
                Ok(parsed) => return parsed,
                Err(_) => last_problem = Some("响应解析失败 / Failed to parse response".into()),
            },
            Ok(reply) => {
                last_problem = Some(format!("状态 {} / status {}", reply.status().as_u16(), reply.status().as_u16()));
            }
            Err(e) => last_problem = Some(e.to_string()),
        }

        if attempt < retries {
            std::thread::sleep(rest);
        }
    }

    json!({
        "success": false,
        "error": last_problem.unwrap_or_else(|| "查询失败 / Query failed".into()),
        "transient": true
    })
}

/// 问钥匙好了没。
/// Ask whether the key is ready.
pub fn session_status(ticket: &str) -> Value {
    query(
        &format!("session/status?ticket={}", urlencoding::encode(ticket)),
        3,
        Duration::from_millis(250),
    )
}

/// 问这个链接的情况（是哪种服务、要过几关、还有没有效）。
/// Ask about the link (which service, how many checkpoints, still valid or not).
pub fn session_info(ticket: &str) -> Value {
    query(
        &format!("session/metadata?ticket={}", urlencoding::encode(ticket)),
        3,
        Duration::from_millis(250),
    )
}

// ---------------------------------------------------------------------------
// 判断链接是不是废了 / Deciding whether a link is dead
// ---------------------------------------------------------------------------

/// 服务器说这些话，就是明确告诉你链接不能用了。
/// When the server says any of these, the link is definitely unusable.
pub const DEAD_LINK_WORDS: [&str; 6] = [
    "invalid payload",
    "expired",
    "not found",
    "invalid session",
    "invalid ticket",
    "does not exist",
];

/// 看这个链接还能不能用。
///
/// 返回 `(能用, 不能用的原因)`。
///
/// 拿不准的时候一律当"能用"。宁可白跑一趟，也别把好链接判死 —— 一时的网络问题
/// 跟链接过期，回话可能都是失败，但处理方式完全不同。
///
/// Check whether a link is still usable.
///
/// Returns `(usable, why not)`.
///
/// When in doubt we treat it as usable. Better to waste one attempt than to
/// write off a good link — a momentary network problem and an expired link can
/// both look like a failed reply, but they need opposite handling.
pub fn check_alive(ticket: &str) -> (bool, Option<String>) {
    let info = session_info(ticket);

    if !info.is_object() {
        return (true, None);
    }
    if info.get("success").and_then(Value::as_bool) == Some(true) {
        return (true, None);
    }
    // 一时的问题，不当链接坏了。
    // A momentary problem is not a broken link.
    if info.get("transient").and_then(Value::as_bool) == Some(true) {
        return (true, None);
    }

    if info.get("success").and_then(Value::as_bool) == Some(false) {
        let said = info
            .get("message")
            .or_else(|| info.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let lower = said.to_lowercase();

        if DEAD_LINK_WORDS.iter().any(|w| lower.contains(w)) {
            return (false, Some(said.to_string()));
        }
    }

    (true, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这两串是用 Python 原版跑出来的结果，用来确认加密写得一模一样。
    /// These two strings came out of the original Python version, and confirm our
    /// encryption matches it exactly.
    #[test]
    fn 加密结果跟原版一致_payload_matches_original() {
        let ticket = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";
        let agent = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_3_2 like Mac OS X) \
AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3.1 Mobile/15E148 Safari/604.1";

        let p = build_payload(ticket, 1_725_234_000_123, agent, "390x844");

        assert_eq!(
            p.meta,
            "fe9a9d6b70fc117e44699becbe7e6588e5546774b59a1ae122c415de19492f05355d9c691f4599a98cbf19e7112512efb04b88942117f8895e793419017554e9d01479db082a3976b00acf9ed20532331883aa95871bcef23ba880804f28e63260650ab180a477f6bd97917cf1e9f5d17e703379b9404422440a57fca0506599e62fb538af1d0dcc534daea0b890b7e719c47b3a67eaee572bbbdb4be3418e75c03c44d39f4100f6c637408e9f3dd81f70a9d45eff0c66d3c8c20cb0cc442d62283e84999ae3aa5c5883cf81b3fc"
        );
        assert_eq!(
            p.stream,
            "c3a61be34da86f4dcfdffb3ee452a4d715a36c1ea3749e1342d76f6e70279b9e5187b26160843cc05938168777f86b566d5159664963"
        );
    }

    #[test]
    fn 各种输入都能抠出串_extracts_from_every_form() {
        assert_eq!(
            extract_ticket("https://auth.platorelay.com/a?d=ABC123&x=1"),
            "ABC123"
        );
        assert_eq!(extract_ticket("ABC123"), "ABC123");
        assert_eq!(extract_ticket("  ABC123  "), "ABC123", "前后空格要去掉 / spaces should be trimmed");
    }

    #[test]
    fn 从文件读也行_reads_from_a_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().expect("临时目录 / temp dir");
        let path = dir.path().join("ticket.txt");
        let mut f = std::fs::File::create(&path).expect("建文件 / create file");
        writeln!(f, "FROMFILE123").expect("写文件 / write file");

        assert_eq!(extract_ticket(path.to_str().unwrap()), "FROMFILE123");
    }

    #[test]
    fn 两种base64都能解_both_base64_styles_decode() {
        use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};

        let target = "https://example.com/next?d=NEXTTICKET_LONG_ENOUGH_TO_LOOK_REAL";

        // 换过字符、不带补位的写法。
        // The swapped-character form without padding.
        let a = format!("https://ads.example/x?r={}", URL_SAFE_NO_PAD.encode(target));
        assert_eq!(decode_callback(&a).as_deref(), Some(target));

        // 标准写法带补位，而且斜杠被转义过（实际见过的形式）。
        // Standard form with padding and escaped slashes, as actually seen.
        let b = format!(
            "https://link-to.net/1/dynamic/?s=0&r={}",
            urlencoding::encode(&STANDARD.encode(target))
        );
        assert_eq!(decode_callback(&b).as_deref(), Some(target));

        assert_eq!(
            ticket_from_callback(target).as_deref(),
            Some("NEXTTICKET_LONG_ENOUGH_TO_LOOK_REAL")
        );
    }

    #[test]
    fn 没有r参数就返回空_missing_r_gives_nothing() {
        assert_eq!(decode_callback("https://ads.example/x?y=1"), None);
        assert_eq!(decode_callback("https://ads.example/x?r="), None);
    }

    #[test]
    fn 转义能挡住引号_escaping_handles_quotes() {
        let messy = r#"a"b\c"#;
        let out = escape(messy);
        assert!(!out.contains(r#"a"b"#), "引号应该被转义 / the quote should be escaped");
        assert!(out.contains(r#"\""#));
    }
}
