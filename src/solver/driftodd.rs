//! driftodd 题：找那个反着转的
//! The driftodd puzzle: find the one going the other way
//!
//! 一堆形状都在绕圈转，其中一个反着转。把每个形状的转向都算出来，数一数：正着转
//! 的几个、反着转的几个，少的那一方就是答案。
//!
//! 下面按顺序试九套办法，从最准的开始。**顺序不能调。** 每套都在前一套失手的情况
//! 下才出手，换个顺序选出来的点就不一样了。
//!
//! A set of shapes all go round, with one of them the other way. Work out which way
//! each one turns and count: so many one way, so many the other, and the smaller
//! group is the answer.
//!
//! Nine methods are tried in order below, best first. **The order must not be
//! changed.** Each one only steps in when the previous failed, and a different order
//! picks a different spot.

use rayon::prelude::*;

use super::tracking::{self, Track};
use super::Choice;
use crate::image::{self, Frames, Grey};

/// 转了这么多以上才算"真的在转"，底下的当抖动。
/// Anything turning more than this counts as really turning; below it is jitter.
const TURNING: f64 = 0.5;

/// 看一张 driftodd 图，选一个点。
/// Look at one driftodd picture and pick a spot.
pub fn solve(frames: &Frames) -> Choice {
    let greys = image::all_grey(frames);

    // ---- 办法一：背景差分找少数派。最准，绝大多数图走这条 ----
    // ---- Method 1: background difference, find the minority. Most accurate,
    //      and nearly every picture goes this way ----
    //
    // 注意：这里不能顺手把办法二一起算了"反正闲着"。办法一命中率很高，办法二那份
    // 计算基本白做，还会把 CPU 抢光，实测反而慢 4 倍。
    //
    // Note: do not compute method 2 alongside "since the cores are idle anyway".
    // Method 1 hits nearly every time, so method 2's work is almost always wasted
    // and it hogs the cores — measured four times slower that way.
    let background_tracks = tracking::track_by_background(frames, 12.0, 15, 35.0, &greys);
    if let Some(pick) = minority_of(&background_tracks) {
        return (Some((pick.x, pick.y)), "背景差分-少数方向 / BackgroundSubtraction-MinorityDirection");
    }

    // ---- 办法二：切格子看转动，找反向那格 ----
    // ---- Method 2: grid of turning strength, find the square going the other way ----
    let (turning_per_square, dots_per_square) = super::coherence::turning_grid(frames, 6, 170.0, &greys);
    if dots_per_square.iter().max().copied().unwrap_or(0) >= 10 {
        let main_direction = if turning_per_square.iter().sum::<f64>() >= 0.0 { 1.0 } else { -1.0 };

        let found = opposite_square(
            &turning_per_square,
            &dots_per_square,
            main_direction,
            frames,
            &greys,
            6,
            170.0,
        );
        if let Some((x, y)) = found {
            return (Some((x, y)), "角动量栅格-主判定 / AngularMomentumGrid-Primary");
        }
    }

    // ---- 办法三：几个明暗门槛都试一遍，找少数派 ----
    // ---- Method 3: try several brightness cut-offs, find the minority ----
    let mut shapes: Vec<Track> = [100.0f32, 120.0, 140.0, 170.0]
        .par_iter()
        .flat_map_iter(|&cutoff| {
            tracking::track_by_cutoff(frames, cutoff, 10, 35.0, &greys).into_iter()
        })
        .collect();

    // 太细的条状不要，那种一般是背景纹理不是形状。
    // Drop the thin streaks; those are usually background texture, not shapes.
    shapes.retain(|s| s.solidity >= 0.15);

    // 按门槛从低到高、同门槛下可信度从高到低排。排序影响去重时留下哪个，得跟原版一致。
    // Sort by cut-off low to high, and within one cut-off by trust high to low. The
    // order decides which one survives de-duplication, so it must match the original.
    shapes.sort_by(|a, b| {
        a.cutoff
            .partial_cmp(&b.cutoff)
            .unwrap()
            .then((-a.confidence).partial_cmp(&(-b.confidence)).unwrap())
    });

    // 位置太近的算同一个形状，只留先出现的那个。
    // Shapes too close together are the same thing; keep only the first.
    let mut distinct: Vec<Track> = Vec::new();
    for s in shapes {
        let already_have = distinct
            .iter()
            .any(|d| ((s.x - d.x).powi(2) + (s.y - d.y).powi(2)).sqrt() < 20.0);
        if !already_have {
            distinct.push(s);
        }
    }

    // 只找到一个形状没法比方向，放弃。
    // With only one shape there is nothing to compare, so give up.
    if distinct.len() < 2 {
        return (None, "候选形状不足 / Insufficient shape candidates");
    }

    let turning: Vec<Track> = distinct
        .iter()
        .filter(|s| s.total_turn.abs() > TURNING)
        .copied()
        .collect();

    // 一个都没在转：那题目大概是"找唯一没在动的"，挑最可信的那个。
    // None of them turning: the puzzle is probably "find the only still one", so
    // take the most trustworthy.
    if turning.is_empty() {
        let candidates: Vec<&Track> = distinct.iter().filter(|s| s.confidence > 0.02).collect();
        if let Some(pick) = image::pick_max(&candidates, |s| s.confidence) {
            return (Some((pick.x, pick.y)), "静止候选-最高置信 / StationaryCandidate-HighestConfidence");
        }
        return (None, "未检测到旋转 / No rotation detected");
    }

    let forward: Vec<&Track> = turning.iter().filter(|s| s.total_turn > 0.0).collect();
    let backward: Vec<&Track> = turning.iter().filter(|s| s.total_turn < 0.0).collect();

    // ---- 办法四：两个方向都有，人少的那一方就是答案 ----
    // ---- Method 4: both directions present, so the smaller group is the answer ----
    if forward.len() != backward.len() && !forward.is_empty() && !backward.is_empty() {
        let fewer = if backward.len() < forward.len() { &backward } else { &forward };

        // 少数派里也得挑像样的：转够多、够可信、不是那种低门槛下才冒出来的小碎片。
        // Even within the minority, pick a proper one: turning enough, trustworthy
        // enough, and not a scrap that only shows up at a low cut-off.
        let proper: Vec<&&Track> = fewer
            .iter()
            .filter(|s| {
                s.total_turn.abs() > 1.5
                    && s.confidence > 0.1
                    && (s.cutoff <= 140.0 || s.area >= 50.0)
            })
            .collect();

        if let Some(pick) = image::pick_max(&proper, |s| s.confidence) {
            return (Some((pick.x, pick.y)), "阈值追踪-少数方向 / ThresholdTracking-MinorityDirection");
        }
    }

    // ---- 办法五：按可信度投票定主方向，再找反着来的 ----
    // ---- Method 5: vote on the main direction by trust, then find one going
    //      against it ----
    //
    // 为什么投票而不是数个数：有的形状转得含糊，可信度低，不该跟转得很干脆的算
    // 一样的票。
    //
    // Why vote rather than count heads: some shapes turn vaguely and score low on
    // trust, and should not weigh the same as one turning decisively.
    let vote: f64 = turning
        .iter()
        .map(|s| image::direction(s.total_turn) * s.confidence)
        .sum();
    let main_direction = if vote >= 0.0 { 1.0 } else { -1.0 };

    let against: Vec<&Track> = distinct
        .iter()
        .filter(|s| {
            image::direction(s.total_turn) == -main_direction
                && s.total_turn.abs() > 1.0
                && s.confidence > 0.1
                && (s.cutoff <= 140.0 || s.area >= 50.0)
        })
        .collect();

    if let Some(pick) = image::pick_max(&against, |s| s.total_turn.abs() * s.confidence) {
        return (Some((pick.x, pick.y)), "逆向位移-最强 / CounterRotation-Strongest");
    }

    // ---- 办法六：调低门槛再扫一遍，看有没有漏掉的 ----
    // ---- Method 6: rescan with lower cut-offs, in case something was missed ----
    //
    // 前面的门槛偏亮，颜色很深的形状可能被并进背景里了。
    //
    // The earlier cut-offs lean bright, so a very dark shape may have been lumped in
    // with the background.
    let low_cutoff: Vec<Track> = [80.0f32, 90.0]
        .par_iter()
        .flat_map_iter(|&cutoff| {
            tracking::track_by_cutoff(frames, cutoff, 8, 35.0, &greys).into_iter()
        })
        .collect();

    let newly_found: Vec<&Track> = low_cutoff
        .iter()
        .filter(|s| s.solidity >= 0.12)
        // 跟已经找到的位置重合的不算新的。
        // Ones overlapping what we already found do not count as new.
        .filter(|s| {
            !distinct
                .iter()
                .any(|d| ((s.x - d.x).powi(2) + (s.y - d.y).powi(2)).sqrt() < 25.0)
        })
        .filter(|s| image::direction(s.total_turn) == -main_direction && s.total_turn.abs() > 1.0)
        .collect();

    if let Some(pick) = image::pick_max(&newly_found, |s| s.total_turn.abs() * s.confidence) {
        return (Some((pick.x, pick.y)), "低阈值重扫-逆向 / LowThresholdRescan-CounterRotation");
    }

    // ---- 办法七：回头看背景差分那批里有没有反向的 ----
    // ---- Method 7: look back at the background batch for anything going against ----
    let mut background_against: Vec<&Track> = background_tracks
        .iter()
        .filter(|s| {
            image::direction(s.total_turn) == -main_direction
                && s.total_turn.abs() > 3.0
                && s.confidence > 0.5
                && s.area >= 40.0
        })
        .collect();

    // 按"转得多且可信"从强到弱排，然后去重。
    // Sort by "turns a lot and is trustworthy", strongest first, then de-duplicate.
    background_against.sort_by(|a, b| {
        (-a.total_turn.abs() * a.confidence)
            .partial_cmp(&(-b.total_turn.abs() * b.confidence))
            .unwrap()
    });

    let mut background_distinct: Vec<&Track> = Vec::new();
    for s in background_against {
        let already_have = background_distinct
            .iter()
            .any(|d| ((s.x - d.x).powi(2) + (s.y - d.y).powi(2)).sqrt() < 25.0);
        if !already_have {
            background_distinct.push(s);
        }
    }

    if let Some(pick) = background_distinct.first() {
        return (Some((pick.x, pick.y)), "背景差分-逆向 / BackgroundSubtraction-CounterRotation");
    }

    // ---- 办法八：格子转动再看一次（这次带上主方向） ----
    // ---- Method 8: check the turning grid again, this time knowing the main
    //      direction ----
    let found = opposite_square(
        &turning_per_square,
        &dots_per_square,
        main_direction,
        frames,
        &greys,
        6,
        170.0,
    );
    if let Some((x, y)) = found {
        return (Some((x, y)), "角动量栅格-逆向 / AngularMomentumGrid-CounterRotation");
    }

    // ---- 办法九组：都不行了，按特征猜 ----
    // ---- Method 9 group: nothing worked, so guess from features ----

    // 特别大的那个可能就是答案（反转的那个常被画得大一些）。
    // The notably big one may be it (the reversed shape is often drawn larger).
    if turning.len() >= 3 {
        let areas: Vec<f64> = turning.iter().map(|s| s.area).collect();
        let typical = image::middle_value(&areas);
        let oversized: Vec<&Track> = turning.iter().filter(|s| s.area > typical * 3.0).collect();
        if let Some(pick) = image::pick_max(&oversized, |s| s.area) {
            return (Some((pick.x, pick.y)), "面积离群 / AreaOutlier");
        }
    }

    // 转得最含糊的那个。反着转的常常被同向的挤得转不利索。
    // The one turning most vaguely. A reversed shape often gets jostled by the ones
    // going the other way.
    let vague: Vec<&Track> = turning
        .iter()
        .filter(|s| s.confidence < 0.5 && s.area >= 50.0)
        .collect();
    if let Some(pick) = image::pick_min(&vague, |s| s.confidence) {
        return (Some((pick.x, pick.y)), "最低一致性 / LowestConsistency");
    }

    // 实在没办法了，挑最可信的那个交上去。总比不点好。
    // Out of options, so submit the most trustworthy one. Better than not clicking.
    if let Some(pick) = image::pick_max(&turning, |s| s.confidence) {
        return (Some((pick.x, pick.y)), "兜底-最高置信 / Fallback-HighestConfidence");
    }

    (None, "所有判定策略均未命中 / All strategies exhausted")
}

/// 从一批轨迹里找少数派。
///
/// 先把"转得够多、够可信、够大"的挑出来，去重，然后比正反两边谁少。两边一样多，
/// 或者有一边是空的，就说明这批看不出少数派，返回空。
///
/// Find the minority in a batch of trails.
///
/// First keep the ones turning enough, trustworthy enough and big enough, then
/// de-duplicate, then compare the two directions. Equal numbers, or one side empty,
/// means no minority can be told apart, so nothing is returned.
fn minority_of(tracks: &[Track]) -> Option<Track> {
    let mut strong: Vec<&Track> = tracks
        .iter()
        .filter(|s| s.total_turn.abs() > 4.0 && s.confidence > 0.3 && s.area >= 40.0)
        .collect();

    strong.sort_by(|a, b| {
        (-a.total_turn.abs() * a.confidence)
            .partial_cmp(&(-b.total_turn.abs() * b.confidence))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut distinct: Vec<&Track> = Vec::new();
    for s in strong {
        let already_have = distinct
            .iter()
            .any(|d| ((s.x - d.x).powi(2) + (s.y - d.y).powi(2)).sqrt() < 30.0);
        if !already_have {
            distinct.push(s);
        }
    }

    let forward: Vec<Track> = distinct
        .iter()
        .filter(|s| s.total_turn > 0.0)
        .map(|s| **s)
        .collect();
    let backward: Vec<Track> = distinct
        .iter()
        .filter(|s| s.total_turn < 0.0)
        .map(|s| **s)
        .collect();

    if distinct.is_empty()
        || forward.len() == backward.len()
        || forward.is_empty()
        || backward.is_empty()
    {
        return None;
    }

    let fewer = if backward.len() < forward.len() { backward } else { forward };
    image::pick_max(&fewer, |s| s.confidence)
}

/// 在格子里找转向跟大流相反、劲头最足的那格，返回那格里暗点的中心。
///
/// 只看中间那些格 —— 最外圈的形状常被画面边缘裁掉，位置算不准。
///
/// Find the square turning against the flow most strongly, and give the middle of
/// its dark dots.
///
/// Only the inner squares are considered — shapes on the outer ring are often cut
/// off by the picture edge, so their position is unreliable.
#[allow(clippy::too_many_arguments)]
fn opposite_square(
    turning_per_square: &[f64],
    dots_per_square: &[i64],
    main_direction: f64,
    frames: &Frames,
    greys: &[Grey],
    grid: usize,
    cutoff: f32,
) -> Option<(f64, f64)> {
    let width = frames.width;
    let height = frames.height;
    let square_w = width / grid;
    let square_h = height / grid;

    let mut best_strength = 0.0f64;
    let mut best_square: Option<(usize, usize)> = None;

    for down in 1..grid.saturating_sub(1) {
        for across in 1..grid.saturating_sub(1) {
            let here = down * grid + across;

            // 点太少的格子不算，样本不够说明不了问题。
            // Squares with too few dots do not count; too small a sample says nothing.
            if dots_per_square[here] < 10 {
                continue;
            }

            let strength = turning_per_square[here];
            if image::direction(strength) == -main_direction
                && strength.abs() > best_strength.abs()
            {
                best_strength = strength;
                best_square = Some((across, down));
            }
        }
    }

    let (across, down) = best_square?;

    // 劲头太弱就不算，可能只是噪声。
    // Too weak to count; it may just be noise.
    if best_strength.abs() < 0.5 {
        return None;
    }

    let left = across * square_w;
    let right = ((across + 1) * square_w).min(width);
    let top = down * square_h;
    let bottom = ((down + 1) * square_h).min(height);

    match square_dark_middle(greys, width, height, left, right, top, bottom, cutoff, 5) {
        // 算出了暗点中心就用它，更准。
        // If a dark-dot middle was worked out, use it; it is more accurate.
        Some(point) => Some(point),
        // 算不出就退回格子中心。
        // Otherwise fall back to the middle of the square.
        None => Some((
            (across * square_w) as f64 + square_w as f64 / 2.0,
            (down * square_h) as f64 + square_h as f64 / 2.0,
        )),
    }
}

/// 一个格子里暗点的中心位置。
///
/// 每张画面各算一个中心，最后取这些中心的中间值 —— 这样个别画面的抖动不影响结果。
///
/// The middle of the dark dots inside one square.
///
/// A middle is worked out for each picture, and the middle of those is taken — so a
/// wobble on one picture does not sway the result.
#[allow(clippy::too_many_arguments)]
fn square_dark_middle(
    greys: &[Grey],
    width: usize,
    height: usize,
    left: usize,
    right: usize,
    top: usize,
    bottom: usize,
    cutoff: f32,
    least_dots: usize,
) -> Option<(f64, f64)> {
    let middles: Vec<(f64, f64)> = greys
        .par_iter()
        .filter_map(|g| {
            let mut sum_x = 0.0f64;
            let mut sum_y = 0.0f64;
            let mut how_many = 0usize;

            for y in top..bottom.min(height) {
                let row = y * width;
                for x in left..right.min(width) {
                    if g.values[row + x] < cutoff {
                        sum_x += x as f64;
                        sum_y += y as f64;
                        how_many += 1;
                    }
                }
            }

            if how_many < least_dots {
                None
            } else {
                Some((sum_x / how_many as f64, sum_y / how_many as f64))
            }
        })
        .collect();

    if middles.is_empty() {
        return None;
    }

    let xs: Vec<f64> = middles.iter().map(|m| m.0).collect();
    let ys: Vec<f64> = middles.iter().map(|m| m.1).collect();
    Some((image::middle_value(&xs), image::middle_value(&ys)))
}
