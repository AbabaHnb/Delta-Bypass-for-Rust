//! 生成测试链接
//! Make test links
//!
//! 自己手上没链接的时候，可以让上游发一条新的来试。跟绕过流程无关，只是方便测试。
//!
//! When you have no link on hand, you can ask the far end for a fresh one to try
//! with. Nothing to do with the bypass itself; it is just for convenience.

use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config;

/// 服务编号：安卓。
/// Service number for Android.
pub const SERVICE_ANDROID: u32 = 6;

/// 服务编号：苹果。
/// Service number for iOS.
pub const SERVICE_IOS: u32 = 8;

/// 复用一个连接，别每次都重新握手。
/// Reuse one connection instead of shaking hands every time.
fn client() -> &'static reqwest::blocking::Client {
    static C: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("初始化链接客户端 / initialise the link client")
    })
}

/// 随机编一个设备号。
///
/// 上游要一个"这是哪台机器"的编号。测试链接用不着真的设备号，随机一个就行。
///
/// Make up a random device id.
///
/// The far end wants an id saying "which machine is this". For test links a real
/// one is not needed, so a random value does.
fn random_device_id() -> String {
    let n: u64 = rand::random();
    let mut hasher = Sha256::new();
    hasher.update(n.to_string().as_bytes());
    hex::encode(hasher.finalize())
}

/// 要一条新链接。
///
/// `service` 用上面的两个常量。`device_id` 留空就随机编一个。
///
/// Ask for one new link.
///
/// Use the constants above for `service`. Leave `device_id` empty and one gets
/// made up.
pub fn create(service: u32, device_id: Option<&str>) -> anyhow::Result<String> {
    let id = device_id
        .map(|s| s.to_string())
        .unwrap_or_else(random_device_id);

    let body = serde_json::json!({ "service": service, "identifier": id });

    let reply: Value = client()
        .post(format!("{}/public/start", config::LINK_API))
        .json(&body)
        .header("User-Agent", config::LINK_AGENT)
        .header("Content-Type", "application/json")
        .send()?
        .json()?;

    if reply.get("success").and_then(Value::as_bool) == Some(true) {
        if let Some(url) = reply["data"]["url"].as_str() {
            return Ok(url.to_string());
        }
    }

    let why = reply
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("未提供原因 / no reason given");
    anyhow::bail!("上游拒绝请求 / Upstream refused the request: {}", why)
}

/// 一次要好几条。
///
/// 中间留点间隔，别一股脑打过去。某一条失败就跳过，不影响其他的。
///
/// Ask for several at once.
///
/// A pause is left between requests rather than firing them all at once. If one
/// fails it is skipped; the rest carry on.
pub fn create_many(count: usize, service: u32, gap: Duration) -> Vec<String> {
    let mut out = Vec::with_capacity(count);

    for i in 0..count {
        match create(service, None) {
            Ok(url) => {
                let shown = &url[..url.len().min(64)];
                eprintln!("[{}/{}] {}...", i + 1, count, shown);
                out.push(url);
            }
            Err(e) => eprintln!("[{}/{}] 失败 / Failed: {}", i + 1, count, e),
        }

        if i + 1 < count {
            std::thread::sleep(gap);
        }
    }

    out
}

/// 把 "android" / "ios" 这样的名字翻成编号。
/// Turn a name like "android" or "ios" into its number.
pub fn service_by_name(name: &str) -> Option<u32> {
    match name.to_lowercase().as_str() {
        "android" => Some(SERVICE_ANDROID),
        "ios" => Some(SERVICE_IOS),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 设备号长度对且每次不同_device_id_is_right_length_and_varies() {
        let a = random_device_id();
        let b = random_device_id();

        // SHA-256 写成十六进制就是 64 个字符。
        // SHA-256 written as hex is 64 characters.
        assert_eq!(a.len(), 64);
        assert_ne!(a, b, "两次应该不一样 / two calls should differ");
    }

    #[test]
    fn 服务名翻译正确_service_names_map_correctly() {
        assert_eq!(service_by_name("android"), Some(SERVICE_ANDROID));
        assert_eq!(service_by_name("ANDROID"), Some(SERVICE_ANDROID));
        assert_eq!(service_by_name("ios"), Some(SERVICE_IOS));
        assert_eq!(service_by_name("塞班 / symbian"), None);
    }
}
