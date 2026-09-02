//! Delta 绕过器 —— 库入口
//! Delta bypass — library entry point
//!
//! 这个文件只做一件事：把各个模块列出来，让别的程序可以当库来用。
//! This file does one job: list the modules so other programs can use this as a
//! library.
//!
//! 想直接跑命令的话看 `src/main.rs`。
//! If you just want to run the command, look at `src/main.rs`.
//!
//! 模块分工 / What each module does:
//!
//! | 模块 module | 作用 purpose |
//! |---|---|
//! | [`config`] | 所有可调数字放一起 / all the numbers you may want to change |
//! | [`crypto`] | 上游要求的那种加密 / the kind of encryption the far end wants |
//! | [`useragent`] | 假装成手机浏览器 / pretend to be a phone browser |
//! | [`link`] | 生成测试链接 / make test links |
//! | [`auth`] | 跟登录服务器说话 / talk to the login server |
//! | [`net`] | 连接建好放着反复用 / build connections once and keep them |
//! | [`platform`] | Windows 下的路径修正 / path fix for Windows |
//! | [`image`] | 读 GIF、找图形 / read GIFs and find shapes |
//! | [`solver`] | 看图选点 / look at the picture and pick a spot |
//! | [`pool`] | 提前备好验证码 / prepare captcha puzzles in advance |
//! | [`chain`] | 从链接一路走到钥匙 / walk from a link all the way to a key |
//! | [`api`] | 对外的网页接口 / the web interface for other programs |
//! | [`timing`] | 记时间、看哪步慢 / keep time, see which step is slow |

pub mod api;
pub mod auth;
pub mod chain;
pub mod config;
pub mod crypto;
pub mod image;
pub mod link;
pub mod net;
pub mod platform;
pub mod pool;
pub mod solver;
pub mod timing;
pub mod useragent;

/// 版本号，跟 Cargo.toml 里保持一致。
/// Version number, kept in step with Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
