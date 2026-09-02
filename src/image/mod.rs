//! 读图片、找形状 —— 基础工具
//! Reading pictures and finding shapes — the basic tools
//!
//! 这里放的都是不带判断的"基本功"：把 GIF 读成一张张画面、把彩色变灰度、把挨在
//! 一起的暗点圈成一块、找离某点最近的点、给一圈点配个圆。
//!
//! 真正"选哪个点"的判断在 [`crate::solver`] 里。
//!
//! 有几处写法看着绕，是为了跟 Python 原版算出一模一样的数。哪里不能改都写了原因，
//! 别顺手"优化"掉。
//!
//! This file holds the plain groundwork, with no judgement calls: turn a GIF into
//! separate pictures, turn colour into grey, group touching dark dots into
//! patches, find the nearest dot to a point, fit a circle through a ring of dots.
//!
//! The actual "which dot do we pick" decisions live in [`crate::solver`].
//!
//! A few things are written awkwardly on purpose, to get exactly the same numbers
//! as the original Python version. Where something must not change, the reason is
//! written down — please do not tidy it away.

use std::io::Cursor;

use gif::{ColorOutput, DecodeOptions, DisposalMethod};
use rayon::prelude::*;

pub mod nearest;
pub mod patches;

pub use nearest::NearestFinder;
pub use patches::{
    dark_by_brightness, dark_by_colour, dark_dots, dark_middles, find as find_patches, middles,
    Box2D, Patches,
};

/// 一段动画，拆成一张张画面。
/// An animation, split into separate pictures.
pub struct Frames {
    /// 宽。
    /// Width.
    pub width: usize,
    /// 高。
    /// Height.
    pub height: usize,
    /// 每张画面的像素，每个像素 4 个字节（红绿蓝 + 透明度）。
    /// The pixels of each picture, 4 bytes per pixel (red, green, blue, see-through).
    pub pictures: Vec<Vec<u8>>,
}

impl Frames {
    /// 一共几张。
    /// How many pictures there are.
    pub fn count(&self) -> usize {
        self.pictures.len()
    }
}

/// 一张灰度画面：每个像素只有明暗，没有颜色。
/// One grey picture: each pixel has only brightness, no colour.
pub struct Grey {
    pub width: usize,
    pub height: usize,
    /// 明暗值，0 是纯黑，255 是纯白。
    /// Brightness values: 0 is black, 255 is white.
    pub values: Vec<f32>,
}

impl Grey {
    /// 取某个位置的明暗值。
    /// Read the brightness at one spot.
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.values[y * self.width + x]
    }
}

// ---------------------------------------------------------------------------
// 读 GIF / Reading a GIF
// ---------------------------------------------------------------------------

/// 把 GIF 读成一张张完整画面。
///
/// **这里最容易出错，务必看完。** GIF 为了省体积，第二张之后往往只存"跟上一张
/// 比，变了的那一小块"。所以必须准备一块画布，每张只把变化的部分盖上去，然后把
/// 整块画布拷一份作为这一张的结果。
///
/// 要是图省事只把每张自己那小块读出来，第一张之后就基本全黑了 —— 后面所有找形
/// 状的步骤全废。
///
/// 另外还要照顾三种"这张画完之后怎么处理"的规矩：什么都不动、把自己那块擦掉、
/// 恢复成上一张的样子。
///
/// Turn a GIF into complete pictures.
///
/// **This is the easiest place to get wrong, so please read it all.** To save
/// space, GIFs often store only "the small part that changed since last time" from
/// the second picture onwards. So we keep a canvas, paste each change onto it, and
/// copy the whole canvas out as that picture's result.
///
/// Take the shortcut of reading only each picture's own small part and everything
/// after the first comes out mostly black — which ruins every shape-finding step
/// that follows.
///
/// Three "what to do once this picture is shown" rules also need handling: leave
/// things be, wipe its own area, or put back what was there before.
pub fn read_gif(bytes: &[u8]) -> anyhow::Result<Frames> {
    let mut options = DecodeOptions::new();
    options.set_color_output(ColorOutput::RGBA);
    let mut reader = options.read_info(Cursor::new(bytes))?;

    let width = reader.width() as usize;
    let height = reader.height() as usize;

    let mut canvas = vec![0u8; width * height * 4];
    let mut pictures = Vec::new();

    while let Some(piece) = reader.read_next_frame()? {
        let piece_w = piece.width as usize;
        let piece_h = piece.height as usize;
        let left = piece.left as usize;
        let top = piece.top as usize;

        // 规矩是"恢复成上一张"的话，先把当前画布存一份。
        // If the rule is "put back what was there", save the canvas first.
        let saved = if piece.dispose == DisposalMethod::Previous {
            Some(canvas.clone())
        } else {
            None
        };

        // 把这一小块盖到画布上。透明的点不盖，让下面原来的内容露出来。
        // Paste this small part onto the canvas. See-through dots are skipped, so
        // whatever was underneath shows.
        for row in 0..piece_h {
            let y = top + row;
            if y >= height {
                continue;
            }
            let src_row = row * piece_w * 4;
            if src_row + piece_w * 4 > piece.buffer.len() {
                continue;
            }
            for col in 0..piece_w {
                let x = left + col;
                if x >= width {
                    continue;
                }
                let src = src_row + col * 4;
                if piece.buffer[src + 3] == 0 {
                    continue;
                }
                let dst = (y * width + x) * 4;
                canvas[dst..dst + 4].copy_from_slice(&piece.buffer[src..src + 4]);
            }
        }

        // 整块画布拷一份，这才是完整的一张画面。
        // Copy the whole canvas out; that is the complete picture.
        pictures.push(canvas.clone());

        match piece.dispose {
            // 把自己那块擦成透明。
            // Wipe its own area back to see-through.
            DisposalMethod::Background => {
                for row in 0..piece_h {
                    let y = top + row;
                    if y >= height {
                        continue;
                    }
                    for col in 0..piece_w {
                        let x = left + col;
                        if x >= width {
                            continue;
                        }
                        let dst = (y * width + x) * 4;
                        canvas[dst..dst + 4].copy_from_slice(&[0, 0, 0, 0]);
                    }
                }
            }
            // 恢复成刚才存的那份。
            // Put back the copy we saved.
            DisposalMethod::Previous => {
                if let Some(old) = saved {
                    canvas = old;
                }
            }
            // 什么都不动，留给下一张继续盖。
            // Leave things be, for the next picture to paste over.
            _ => {}
        }
    }

    Ok(Frames { width, height, pictures })
}

// ---------------------------------------------------------------------------
// 彩色变灰度 / Colour to grey
// ---------------------------------------------------------------------------

/// 把一张彩色画面变成灰度。
///
/// **这里的除法不能改成乘法。** 红绿蓝加起来除以 3，必须老老实实写除以 3.0。
/// 有人会想"除法慢，改成乘 1/3 更快" —— 不行。举个例子：510 除以 3 正好是 170.0，
/// 但 510 乘 1/3 算出来是 169.99998。而判断"这个点算不算暗"的门槛恰好是 170：
/// 一个是"不算暗"，一个是"算暗"，结果就反了。
///
/// 一个像素翻了，圈出来的形状就变了，后面选点也就跟着变。这是实测踩过的坑。
///
/// Turn one colour picture into grey.
///
/// **The division here must not become a multiplication.** Add red, green and
/// blue, then genuinely divide by 3.0. Someone will want to say "division is slow,
/// multiply by 1/3 instead" — no. For example: 510 divided by 3 is exactly 170.0,
/// but 510 times 1/3 comes out as 169.99998. And the cut-off for "does this dot
/// count as dark" happens to be 170: one says no, the other says yes, so the
/// answer flips.
///
/// Flip one pixel and the patches change shape, and the dot we pick changes with
/// them. This was found the hard way.
pub fn to_grey(pixels: &[u8], width: usize, height: usize) -> Grey {
    let mut values = Vec::with_capacity(width * height);
    for i in 0..width * height {
        let sum = pixels[i * 4] as u16 + pixels[i * 4 + 1] as u16 + pixels[i * 4 + 2] as u16;
        values.push(sum as f32 / 3.0);
    }
    Grey { width, height, values }
}

/// 所有画面一起转灰度，分给多个核同时做。
/// Turn every picture grey at once, split across several CPU cores.
pub fn all_grey(frames: &Frames) -> Vec<Grey> {
    frames
        .pictures
        .par_iter()
        .map(|p| to_grey(p, frames.width, frames.height))
        .collect()
}

/// 算出"不动的背景"。
///
/// 办法很朴素：同一个位置在所有画面上的明暗值排个序，取中间那个。会动的东西只在
/// 少数几张里遮住这个位置，取中间值就把它们排除掉了，剩下的就是背景。
///
/// Work out the background that does not move.
///
/// The method is plain: for one spot, line up its brightness across every picture
/// and take the middle value. Moving things only cover that spot in a few
/// pictures, so the middle value leaves them out and what remains is the
/// background.
pub fn background(greys: &[Grey]) -> Grey {
    let width = greys[0].width;
    let height = greys[0].height;
    let count = greys.len();

    let mut values = vec![0.0f32; width * height];

    values
        .par_iter_mut()
        .enumerate()
        .for_each_init(
            // 每个核自己一块临时地方，省得反复申请。
            // Each core gets its own scratch space, to avoid asking for memory
            // over and over.
            || vec![0.0f32; count],
            |scratch, (spot, out)| {
                for (i, g) in greys.iter().enumerate() {
                    scratch[i] = g.values[spot];
                }
                *out = middle_value_f32(scratch);
            },
        );

    Grey { width, height, values }
}

// ---------------------------------------------------------------------------
// 小算法 / Small helpers
// ---------------------------------------------------------------------------

/// 取一堆数的中间值（会打乱原来的顺序）。
///
/// 偶数个就取中间两个的平均。用的是"只把中间那个挑对位置"的办法，比全排序快。
///
/// Take the middle value of some numbers (the order gets shuffled).
///
/// With an even count it averages the middle two. It only puts the middle item in
/// place rather than sorting everything, which is faster.
pub fn middle_value_f32(numbers: &mut [f32]) -> f32 {
    let n = numbers.len();
    if n == 0 {
        return 0.0;
    }
    let k = n / 2;

    if n % 2 == 1 {
        let (_, mid, _) = numbers.select_nth_unstable_by(k, |a, b| a.partial_cmp(b).unwrap());
        *mid
    } else {
        let (left, mid, _) = numbers.select_nth_unstable_by(k, |a, b| a.partial_cmp(b).unwrap());
        let upper = *mid;
        let lower = left.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        (lower + upper) * 0.5
    }
}

/// 取中间值，但不动原来那份。
/// Take the middle value without disturbing the original.
pub fn middle_value(numbers: &[f64]) -> f64 {
    let n = numbers.len();
    if n == 0 {
        return 0.0;
    }
    let mut copy = numbers.to_vec();
    let k = n / 2;

    if n % 2 == 1 {
        let (_, mid, _) = copy.select_nth_unstable_by(k, |a, b| a.partial_cmp(b).unwrap());
        *mid
    } else {
        let (left, mid, _) = copy.select_nth_unstable_by(k, |a, b| a.partial_cmp(b).unwrap());
        let upper = *mid;
        let lower = left.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (lower + upper) * 0.5
    }
}

/// 看一个数是正、负还是零，返回 1、-1 或 0。
/// Tell whether a number is positive, negative or zero: gives 1, -1 or 0.
pub fn direction(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// 找最大的那个。
///
/// **平手时取先出现的那个。** Rust 自带的 `max_by` 平手取后出现的，跟 Python 相反。
/// 这个差别会让选出来的点不一样，所以自己写一个。
///
/// Find the biggest one.
///
/// **On a tie, take the first one.** Rust's built-in `max_by` takes the last one
/// on a tie, the opposite of Python. That difference changes which dot gets
/// picked, so we write our own.
pub fn pick_max<T: Copy, K: PartialOrd>(items: &[T], score: impl Fn(&T) -> K) -> Option<T> {
    let mut it = items.iter();
    let mut best = *it.next()?;
    for item in it {
        if score(item) > score(&best) {
            best = *item;
        }
    }
    Some(best)
}

/// 找最小的那个，平手同样取先出现的。
/// Find the smallest one; again, on a tie take the first.
pub fn pick_min<T: Copy, K: PartialOrd>(items: &[T], score: impl Fn(&T) -> K) -> Option<T> {
    let mut it = items.iter();
    let mut best = *it.next()?;
    for item in it {
        if score(item) < score(&best) {
            best = *item;
        }
    }
    Some(best)
}

/// 把一串角度理顺。
///
/// 角度转过一整圈会从 3.14 突然跳到 -3.14，看着像倒转了。这个函数把这种跳变补
/// 回去，让角度一路往一个方向长，这样才能看出"总共转了多少"。
///
/// Straighten out a run of angles.
///
/// An angle passing all the way round jumps from 3.14 to -3.14, which looks like
/// it reversed. This puts those jumps back, so the angle keeps growing one way and
/// we can see how far it turned in total.
pub fn unwrap_angles(angles: &[f64]) -> Vec<f64> {
    if angles.is_empty() {
        return vec![];
    }
    let pi = std::f64::consts::PI;
    let full = 2.0 * pi;

    let mut out = Vec::with_capacity(angles.len());
    out.push(angles[0]);

    for i in 1..angles.len() {
        let mut change = angles[i] - angles[i - 1];
        // 把变化量收进 -pi 到 pi 之间，跳变就没了。
        // Bring the change into -pi..pi and the jump disappears.
        change = (change + pi).rem_euclid(full) - pi;
        out.push(out[i - 1] + change);
    }

    out
}

/// 给一圈点配一个圆，返回圆心和半径。
///
/// 用的是最小二乘：找一个圆，让所有点到它的偏差整体最小。半径取各点到圆心距离的
/// 中间值，这样个别偏得远的点影响不大。
///
/// Fit a circle through a ring of dots, giving the centre and radius.
///
/// This is least squares: find the circle where every dot is off by as little as
/// possible overall. The radius is the middle of the dots' distances to the
/// centre, so one dot sitting far out does not skew it much.
pub fn fit_circle(dots: &[(f64, f64)]) -> (f64, f64, f64) {
    if dots.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    let mut m = [[0.0f64; 3]; 3];
    let mut rhs = [0.0f64; 3];

    for &(x, y) in dots {
        let row = [2.0 * x, 2.0 * y, 1.0];
        let value = x * x + y * y;
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += row[i] * row[j];
            }
            rhs[i] += row[i] * value;
        }
    }

    let answer = solve_three(m, rhs);
    let (cx, cy) = (answer[0], answer[1]);

    let distances: Vec<f64> = dots
        .iter()
        .map(|&(x, y)| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt())
        .collect();

    (cx, cy, middle_value(&distances))
}

/// 解三个未知数的方程组。给 [`fit_circle`] 用。
/// Solve three equations with three unknowns. Used by [`fit_circle`].
fn solve_three(mut m: [[f64; 3]; 3], mut rhs: [f64; 3]) -> [f64; 3] {
    for col in 0..3 {
        // 挑这一列里最大的那行来消，数值上更稳。
        // Pick the biggest row in this column to work with; it is steadier.
        let mut biggest = col;
        for row in col + 1..3 {
            if m[row][col].abs() > m[biggest][col].abs() {
                biggest = row;
            }
        }
        m.swap(col, biggest);
        rhs.swap(col, biggest);

        let pivot = m[col][col];
        if pivot.abs() < 1e-12 {
            continue;
        }

        for row in 0..3 {
            if row != col {
                let factor = m[row][col] / pivot;
                let pivot_row = m[col];
                for (target, source) in m[row].iter_mut().zip(pivot_row.iter()) {
                    *target -= factor * source;
                }
                rhs[row] -= factor * rhs[col];
            }
        }
    }

    let mut out = [0.0f64; 3];
    for i in 0..3 {
        if m[i][i].abs() > 1e-12 {
            out[i] = rhs[i] / m[i][i];
        }
    }
    out
}

/// 用一条直线拟合一组点，返回斜率和截距。
/// Fit a straight line to some points, giving the slope and where it starts.
pub fn fit_line(xs: &[f64], ys: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let mean_x = xs.iter().sum::<f64>() / n;
    let mean_y = ys.iter().sum::<f64>() / n;

    let mut top = 0.0;
    let mut bottom = 0.0;
    for i in 0..xs.len() {
        top += (xs[i] - mean_x) * (ys[i] - mean_y);
        bottom += (xs[i] - mean_x).powi(2);
    }

    let slope = if bottom > 0.0 { top / bottom } else { 0.0 };
    (slope, mean_y - slope * mean_x)
}
