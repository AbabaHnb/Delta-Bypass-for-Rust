//! 看图选点
//! Look at the picture and pick a spot
//!
//! 两种题：
//!
//! **driftodd** —— 一堆形状都在绕圈转，其中一个反着转，点那个。
//! 办法：把每个形状转的方向都算出来，数一数哪个方向的少，少的那个就是答案。
//!
//! **coherence** —— 满屏小点在动，有一小片不动（或者动得少），点那片。
//! 办法：把画面切成格子，数每格里有多少点动了。动得最少的那格就是答案。
//!
//! 每种题都准备了好几套办法，从最准的开始试，不行就换下一套。这是照着原版的顺序
//! 来的，顺序不能乱 —— 换个顺序选出来的点就不一样了。
//!
//! Two kinds of puzzle:
//!
//! **driftodd** — a set of shapes all going round, one of them the other way. Click
//! that one. Method: work out which way each shape turns, count the directions, and
//! whichever direction has fewer shapes is the answer.
//!
//! **coherence** — a screen full of small dots moving, with one patch staying still
//! (or barely moving). Click that patch. Method: cut the picture into a grid and
//! count how many dots moved in each square. The square with the least movement is
//! the answer.
//!
//! Each puzzle has several methods ready, tried best-first and falling through to
//! the next. This follows the original's order, and the order must not be shuffled —
//! a different order picks a different spot.

pub mod coherence;
pub mod driftodd;
pub mod tracking;

pub use tracking::Track;

use crate::image;

/// 选点结果：位置，和用的是哪套办法。
///
/// 位置为空表示没选出来，调用方该重新拿一道题。
///
/// The outcome: a position, and which method produced it.
///
/// An empty position means nothing was chosen, and the caller should get a fresh
/// puzzle.
pub type Choice = (Option<(f64, f64)>, &'static str);

/// 看一张图，选一个点。
///
/// `kind` 是题的种类，服务器会告诉你。认不出的种类返回空。
///
/// Look at one picture and pick a spot.
///
/// `kind` is the puzzle type, which the server tells you. An unknown type gives
/// nothing.
pub fn solve(gif_bytes: &[u8], kind: &str) -> Choice {
    let frames = match image::read_gif(gif_bytes) {
        Ok(f) => f,
        Err(_) => return (None, "图像解码失败 / Image decode failed"),
    };

    if frames.pictures.is_empty() {
        return (None, "图像不含有效帧 / Image contains no usable frames");
    }

    match kind {
        "driftodd" => driftodd::solve(&frames),
        "coherence" => coherence::solve(&frames),
        _ => (None, "未知验证码类型 / Unknown captcha type"),
    }
}
