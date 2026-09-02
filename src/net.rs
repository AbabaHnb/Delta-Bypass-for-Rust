//! 网络连接：建好放着反复用
//! Network connections: build once, keep using
//!
//! 为什么不每次新建一条连接？因为握手很贵。验证码那张 45KB 的 GIF，走已经开好的
//! 连接大约 25 毫秒就下完了，新开一条要 660 毫秒 —— 差 26 倍，全花在握手和"慢启动"
//! 上（新连接一开始不敢快传，要试探着加速）。
//!
//! 所以整个程序共用一份连接，闲着也留着不关。
//!
//! Why not open a fresh connection each time? Because handshakes are expensive. That
//! 45KB puzzle GIF takes about 25 milliseconds over an already-open connection versus
//! 660 over a new one — 26 times the cost, all of it handshaking and "slow start" (a
//! new connection does not dare send fast at first and has to feel its way up).
//!
//! So the whole program shares one set of connections, kept open even while idle.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::config;

/// 建一条连向验证码服务器的连接。
///
/// 带上小饼干（cookie）保管，因为那边靠饼干认"还是刚才那个访客"。
///
/// Build a connection to the puzzle server.
///
/// It keeps cookies, because that side uses them to recognise "still the same
/// visitor".
fn build_captcha_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .cookie_store(true)
        .timeout(config::IMAGE_TIMEOUT)
        .pool_idle_timeout(config::POOL_IDLE_TIMEOUT)
        .pool_max_idle_per_host(8)
        .tcp_keepalive(Duration::from_secs(60))
        .tcp_nodelay(true)
        .build()
        .expect("初始化验证码客户端 / initialise the captcha client")
}

/// 放共用连接的地方。
///
/// 外面套一层锁是为了出问题时能整个换掉（见 [`reset_captcha_client`]）。
///
/// Where the shared connection lives.
///
/// The lock around it is so the whole thing can be swapped out when something goes
/// wrong (see [`reset_captcha_client`]).
fn captcha_slot() -> &'static Mutex<Arc<reqwest::blocking::Client>> {
    static SLOT: OnceLock<Mutex<Arc<reqwest::blocking::Client>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Arc::new(build_captcha_client())))
}

/// 拿共用的验证码连接。
/// Get the shared puzzle connection.
pub fn captcha_client() -> Arc<reqwest::blocking::Client> {
    captcha_slot().lock().unwrap().clone()
}

/// 整个换一条新连接。
///
/// 连接层面出了毛病（比如对方把连接掐了）时用。已经拿走旧的那些还能继续用完，
/// 之后再拿就是新的了。
///
/// Swap in a brand-new connection.
///
/// Used when something goes wrong at the connection level, such as the other side
/// cutting us off. Anyone already holding the old one can finish with it; from then on
/// new callers get the new one.
pub fn reset_captcha_client() {
    *captcha_slot().lock().unwrap() = Arc::new(build_captcha_client());
}

/// 提前把连接开好放着。
///
/// 这样第一个真正的请求不用等握手。故意访问首页而不是拿题的地址 —— 拿题会消耗
/// 一道题，白浪费。
///
/// Open the connection ahead of time.
///
/// That way the first real request skips the handshake. It deliberately visits the
/// home page rather than the fetch-a-puzzle address — fetching would use up a puzzle
/// for nothing.
pub fn warm_up_captcha() {
    let client = captcha_client();
    let _ = client
        .get(format!("{}/", config::CAPTCHA_HOST))
        .timeout(Duration::from_secs(5))
        .send()
        .and_then(|r| r.bytes());
}

/// 两个服务器的连接一起预热。
///
/// 两边同时开，别一个等一个。
///
/// Warm up both servers' connections.
///
/// Both at once, rather than one waiting for the other.
pub fn warm_up_all() {
    let a = std::thread::spawn(warm_up_captcha);
    let b = std::thread::spawn(crate::auth::warm_up);
    let _ = a.join();
    let _ = b.join();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 拿到的是同一份_gives_back_the_same_one() {
        let a = captcha_client();
        let b = captcha_client();
        assert!(
            Arc::ptr_eq(&a, &b),
            "两次应该拿到同一份，不然连接就白留了 / both calls should give the same one, \
             otherwise keeping connections is pointless"
        );
    }

    #[test]
    fn 换过之后就是新的_after_a_swap_it_is_new() {
        let before = captcha_client();
        reset_captcha_client();
        let after = captcha_client();
        assert!(
            !Arc::ptr_eq(&before, &after),
            "换过之后应该是新的一份 / after a swap it should be a different one"
        );
    }
}
