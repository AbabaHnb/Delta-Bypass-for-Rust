//! coherence 题：找那片不动的
//! The coherence puzzle: find the patch that stays still
//!
//! 满屏小点都在动，有一小片不动或者动得很少，点那片。
//!
//! 办法：把画面切成格子，一张一张比对，数每格里有多少点换了位置。动得最少的那格
//! 就是答案所在。
//!
//! 分两轮：先用粗格子（10×8）大致定位，再在那一小片区域用细格子精确定位。一步到位
//! 用细格子不行 —— 格子太小样本太少，噪声就能左右结果。
//!
//! A screen full of small dots is moving, with one patch staying still or barely
//! moving. Click that patch.
//!
//! Method: cut the picture into a grid, compare picture to picture, and count how
//! many dots changed position in each square. The square with the least movement is
//! where the answer is.
//!
//! Two rounds: a coarse grid (10 by 8) to find the rough area, then a fine grid
//! within that small area to pin it down. Going straight to a fine grid does not
//! work — the squares hold too few dots and noise decides the outcome.

use rayon::prelude::*;

use super::Choice;
use crate::image::{self, Frames, Grey, NearestFinder};

/// 点挪了这么远才算"动了"。底下的算原地抖动。
/// A dot must move this far to count as having moved. Less than that is jitter.
const MOVED: f64 = 3.0;

/// 粗格子：横 10 格、竖 8 格。
/// The coarse grid: 10 across, 8 down.
const COARSE_ACROSS: usize = 10;
const COARSE_DOWN: usize = 8;

/// 粗格子的每格再切这么多份做细格子。
/// Each coarse square is cut into this many parts for the fine grid.
const FINE_SPLIT: usize = 4;

/// 看一张 coherence 图，选一个点。
/// Look at one coherence picture and pick a spot.
pub fn solve(frames: &Frames) -> Choice {
    let width = frames.width;
    let height = frames.height;

    // 每张画面上所有小点的位置。这题的点很小，按颜色挑比按明暗挑准。
    // Where every small dot sits on each picture. The dots here are small, and
    // picking by colour works better than by brightness.
    let dots_per_picture: Vec<Vec<(f64, f64)>> = frames
        .pictures
        .par_iter()
        .map(|p| image::dark_middles(p, width, height, true))
        .collect();

    // 格子尺寸用整除，跟原版一致。除不尽剩下的几行几列就不管了。
    // Square sizes use whole-number division, matching the original. Any leftover
    // rows or columns are simply not covered.
    let coarse_w = width / COARSE_ACROSS;
    let coarse_h = height / COARSE_DOWN;

    // ---- 第一轮：粗格子找大致位置 ----
    // ---- Round one: coarse grid for the rough position ----
    let (total, moved) = movement_grid(
        &dots_per_picture,
        coarse_w as f64,
        coarse_h as f64,
        COARSE_ACROSS,
        COARSE_DOWN,
        None,
    );

    let squares = COARSE_ACROSS * COARSE_DOWN;
    let mut move_rate: Vec<f64> = (0..squares)
        .map(|i| {
            // 点太少的格子不作数，直接记成"全动了"排除掉。
            // Squares with too few dots do not count; mark them as "all moved" to
            // rule them out.
            if total[i] > 15 {
                moved[i] as f64 / (total[i] as f64).max(1.0)
            } else {
                1.0
            }
        })
        .collect();

    // 最外圈一律排除。答案不会贴着边，而且边上的点容易进出画面造成误判。
    // Rule out the outer ring entirely. The answer is never against the edge, and
    // dots there drift in and out of view and mislead us.
    for down in 0..COARSE_DOWN {
        for across in 0..COARSE_ACROSS {
            let on_edge = down == 0
                || down == COARSE_DOWN - 1
                || across == 0
                || across == COARSE_ACROSS - 1;
            if on_edge {
                move_rate[down * COARSE_ACROSS + across] = 1.0;
            }
        }
    }

    // 动得最少的那格。平手取先出现的。
    // The square that moved least. On a tie, take the first.
    let mut least_index = 0usize;
    let mut least_rate = f64::INFINITY;
    for (i, &rate) in move_rate.iter().enumerate() {
        if rate < least_rate {
            least_rate = rate;
            least_index = i;
        }
    }
    let coarse_down = least_index / COARSE_ACROSS;
    let coarse_across = least_index % COARSE_ACROSS;

    // ---- 第二轮：在那格及周围一圈里用细格子 ----
    // ---- Round two: fine grid over that square and one ring around it ----
    //
    // 带上周围一圈，因为答案那片可能正好压在两格之间。
    //
    // The ring is included because the answer patch may straddle two squares.
    let from_down = coarse_down.saturating_sub(1);
    let to_down = COARSE_DOWN.min(coarse_down + 2);
    let from_across = coarse_across.saturating_sub(1);
    let to_across = COARSE_ACROSS.min(coarse_across + 2);

    // 细格子尺寸也用整除，跟原版一致。
    // Fine square sizes also use whole-number division, matching the original.
    let fine_w = (width / COARSE_ACROSS) / FINE_SPLIT;
    let fine_h = (height / COARSE_DOWN) / FINE_SPLIT;
    let fine_across = COARSE_ACROSS * FINE_SPLIT;
    let fine_down = COARSE_DOWN * FINE_SPLIT;

    let (fine_total, fine_moved) = movement_grid(
        &dots_per_picture,
        fine_w as f64,
        fine_h as f64,
        fine_across,
        fine_down,
        Some((
            from_down * FINE_SPLIT,
            to_down * FINE_SPLIT,
            from_across * FINE_SPLIT,
            to_across * FINE_SPLIT,
        )),
    );

    let mut fine_rate: Vec<f64> = (0..fine_total.len())
        .map(|i| {
            if fine_total[i] > 3 {
                fine_moved[i] as f64 / (fine_total[i] as f64).max(1.0)
            } else {
                1.0
            }
        })
        .collect();

    // 细格子也要排除边上一圈，以及粗格子选中区域之外的部分。
    // The fine grid also rules out its edge ring, plus anything outside the area the
    // coarse round picked.
    let edge_down = 1.max(36 / fine_h.max(1));
    let edge_across = 1.max(36 / fine_w.max(1));

    let keep_down_from = (coarse_down as isize * FINE_SPLIT as isize - 4).max(0) as usize;
    let keep_down_to = fine_down.min((coarse_down + 1) * FINE_SPLIT + 4);
    let keep_across_from = (coarse_across as isize * FINE_SPLIT as isize - 4).max(0) as usize;
    let keep_across_to = fine_across.min((coarse_across + 1) * FINE_SPLIT + 4);

    for down in 0..fine_down {
        for across in 0..fine_across {
            let on_edge = down < edge_down
                || down >= fine_down - edge_down
                || across < edge_across
                || across >= fine_across - edge_across;
            let outside = down < keep_down_from
                || down >= keep_down_to
                || across < keep_across_from
                || across >= keep_across_to;

            if on_edge || outside {
                fine_rate[down * fine_across + across] = 1.0;
            }
        }
    }

    // 找细格子里动得最少的比率。
    // Find the lowest movement rate among the fine squares.
    let mut lowest = f64::INFINITY;
    for i in 0..fine_total.len() {
        if fine_total[i] >= 4 && fine_rate[i] < lowest {
            lowest = fine_rate[i];
        }
    }

    if lowest.is_finite() {
        // 不只取最低那一格，而是把差不多低的都算进来，按点数加权求中心。
        // 答案那片通常跨几格，只取一格会偏。
        //
        // Rather than just the single lowest square, include all the similarly low
        // ones and take a middle weighted by dot count. The answer patch usually
        // spans several squares, so one square alone would be off-centre.
        let mut weight_total = 0.0f64;
        let mut x_total = 0.0f64;
        let mut y_total = 0.0f64;

        for down in 0..fine_down {
            for across in 0..fine_across {
                let here = down * fine_across + across;
                if fine_rate[here] <= lowest + 0.15 && fine_total[here] >= 4 {
                    let weight = fine_total[here] as f64;
                    weight_total += weight;
                    x_total += (across as f64 * fine_w as f64 + fine_w as f64 / 2.0) * weight;
                    y_total += (down as f64 * fine_h as f64 + fine_h as f64 / 2.0) * weight;
                }
            }
        }

        if weight_total > 0.0 {
            // 往里收 25 像素，别点在最边上。
            // Pull in 25 pixels, so we never click right on the edge.
            let x = (x_total / weight_total).clamp(25.0, (width - 25) as f64);
            let y = (y_total / weight_total).clamp(25.0, (height - 25) as f64);
            return (Some((x, y)), "细栅格-加权质心 / FineGrid-WeightedCentroid");
        }
    }

    // 细格子没结果，退回粗格子中心。
    // The fine grid gave nothing, so fall back to the coarse square's middle.
    let x = (coarse_across as f64 * coarse_w as f64 + coarse_w as f64 / 2.0)
        .clamp(25.0, (width - 25) as f64);
    let y = (coarse_down as f64 * coarse_h as f64 + coarse_h as f64 / 2.0)
        .clamp(25.0, (height - 25) as f64);
    (Some((x, y)), "粗栅格-单元中心 / CoarseGrid-CellCenter")
}

/// 数每格里有多少点、其中多少动了。
///
/// 一张一张往下比：这张的每个点，在下一张里最近的点有多远？超过门槛就算它动了。
///
/// `only_within` 给定的话就只统计那块区域（细格子那轮用），省掉大量无用计算。
///
/// Count how many dots are in each square and how many of them moved.
///
/// Comparing picture to picture: for each dot here, how far is the nearest dot in
/// the next picture? Past the cut-off and it counts as having moved.
///
/// If `only_within` is given, only that area is counted (used by the fine round),
/// which saves a lot of pointless work.
pub fn movement_grid(
    dots_per_picture: &[Vec<(f64, f64)>],
    square_w: f64,
    square_h: f64,
    across_count: usize,
    down_count: usize,
    only_within: Option<(usize, usize, usize, usize)>,
) -> (Vec<i64>, Vec<i64>) {
    let squares = across_count * down_count;
    let pairs: Vec<usize> = (0..dots_per_picture.len().saturating_sub(1)).collect();

    // 每对相邻画面各自数一份，最后加起来。这样可以并行。
    // Each pair of neighbouring pictures is counted separately and added up at the
    // end, which lets it run in parallel.
    pairs
        .into_par_iter()
        .map(|which| {
            let mut total = vec![0i64; squares];
            let mut moved = vec![0i64; squares];

            let here = &dots_per_picture[which];
            let next = &dots_per_picture[which + 1];
            if here.is_empty() || next.is_empty() {
                return (total, moved);
            }

            let finder = NearestFinder::build(next);

            for &(x, y) in here {
                let (_, distance) = finder.nearest((x, y));
                let did_move = distance > MOVED;

                let down = (y / square_h).floor() as isize;
                let across = (x / square_w).floor() as isize;

                if let Some((from_down, to_down, from_across, to_across)) = only_within {
                    let outside = down < from_down as isize
                        || down >= to_down as isize
                        || across < from_across as isize
                        || across >= to_across as isize;
                    if outside {
                        continue;
                    }
                }

                if down < 0
                    || down >= down_count as isize
                    || across < 0
                    || across >= across_count as isize
                {
                    continue;
                }

                let cell = down as usize * across_count + across as usize;
                total[cell] += 1;
                if did_move {
                    moved[cell] += 1;
                }
            }

            (total, moved)
        })
        .reduce(
            || (vec![0i64; squares], vec![0i64; squares]),
            |mut sum, part| {
                for i in 0..squares {
                    sum.0[i] += part.0[i];
                    sum.1[i] += part.1[i];
                }
                sum
            },
        )
}

/// 数每格里的"转动劲头"，给 driftodd 那边用。
///
/// 跟上面数移动不同：这里不光看点动了没有，还看它是"绕着格子中心往哪边转"。
/// 一个点相对格子中心的位置，配上它的移动方向，就能算出是顺时针还是逆时针。
/// 全格加起来，正负就代表这格整体的转向。
///
/// Count the "turning strength" in each square, used by the driftodd side.
///
/// Different from counting movement above: here we look not just at whether a dot
/// moved but which way it went round the square's middle. A dot's position relative
/// to that middle, together with the direction it moved, tells us clockwise or
/// anticlockwise. Added up across a square, the sign shows which way the square
/// turns overall.
pub fn turning_grid(
    frames: &Frames,
    grid: usize,
    cutoff: f32,
    greys: &[Grey],
) -> (Vec<f64>, Vec<i64>) {
    let width = frames.width;
    let height = frames.height;
    let square_h = height / grid;
    let square_w = width / grid;
    let squares = grid * grid;

    // 每张画面上的暗点位置，两轮比对都要用，先算好。
    // The dark dot positions on each picture, needed by both comparisons, so work
    // them out once.
    let dots_per_picture: Vec<Vec<(f64, f64)>> =
        greys.par_iter().map(|g| image::dark_dots(g, cutoff)).collect();

    let pairs: Vec<usize> = (0..greys.len().saturating_sub(1)).collect();

    let (turning, dot_count) = pairs
        .into_par_iter()
        .map(|which| {
            let mut turning = vec![0.0f64; squares];
            let mut counted = vec![0i64; squares];

            let here = &dots_per_picture[which];
            let next = &dots_per_picture[which + 1];

            // 点太少说不出问题，这对跳过。
            // Too few dots to tell anything, so skip this pair.
            if here.len() < 10 || next.len() < 10 {
                return (turning, counted);
            }

            let finder = NearestFinder::build(next);

            // 先攒着不直接加。这对里动的点太少的话整对都不算 —— 跟原版一致。
            // Gather rather than adding straight away. If too few dots moved in this
            // pair, the whole pair is discarded — matching the original.
            let mut gathered: Vec<(usize, f64)> = Vec::new();

            for &(x, y) in here {
                let (index, distance) = finder.nearest((x, y));

                // 几乎没动的点对判断转向没帮助。
                // Dots that barely moved say nothing about which way things turn.
                if distance <= 1.5 {
                    continue;
                }

                let (next_x, next_y) = next[index];
                let move_x = next_x - x;
                let move_y = next_y - y;

                let across = ((x as usize) / square_w).min(grid - 1);
                let down = ((y as usize) / square_h).min(grid - 1);

                let square_middle_x = (across * square_w) as f64 + square_w as f64 / 2.0;
                let square_middle_y = (down * square_h) as f64 + square_h as f64 / 2.0;

                // 位置和移动方向一叉乘，正负就是转向。
                // Cross the position with the movement direction, and the sign gives
                // the turning direction.
                let strength = (x - square_middle_x) * move_y - (y - square_middle_y) * move_x;

                gathered.push((down * grid + across, strength));
            }

            if gathered.len() >= 5 {
                for (cell, strength) in gathered {
                    turning[cell] += strength;
                    counted[cell] += 1;
                }
            }

            (turning, counted)
        })
        .reduce(
            || (vec![0.0f64; squares], vec![0i64; squares]),
            |mut sum, part| {
                for i in 0..squares {
                    sum.0[i] += part.0[i];
                    sum.1[i] += part.1[i];
                }
                sum
            },
        );

    // 除以点数，这样点多点少的格子能公平比较。点太少的格子记 0 排除掉。
    // Divide by the dot count so busy and sparse squares compare fairly. Squares with
    // too few dots record 0 and are ruled out.
    let averaged: Vec<f64> = (0..squares)
        .map(|i| {
            if dot_count[i] > 5 {
                turning[i] / dot_count[i] as f64
            } else {
                0.0
            }
        })
        .collect();

    (averaged, dot_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 不动的点不算动_still_dots_are_not_counted_as_moved() {
        // 三张画面，点都在原地。
        // Three pictures with the dots staying put.
        let same = vec![(10.0, 10.0), (20.0, 20.0), (30.0, 30.0)];
        let pictures = vec![same.clone(), same.clone(), same];

        let (total, moved) = movement_grid(&pictures, 40.0, 40.0, 2, 2, None);

        assert!(total.iter().sum::<i64>() > 0, "应该数到点 / dots should be counted");
        assert_eq!(moved.iter().sum::<i64>(), 0, "没动就不该记成动 / nothing moved, so nothing counted");
    }

    #[test]
    fn 动了的会被数出来_moving_dots_are_counted() {
        // 点每张都往右挪一大截。
        // The dots shift a long way right in each picture.
        let first = vec![(10.0, 10.0), (12.0, 10.0)];
        let second = vec![(60.0, 10.0), (62.0, 10.0)];
        let pictures = vec![first, second];

        let (total, moved) = movement_grid(&pictures, 100.0, 100.0, 1, 1, None);

        assert_eq!(total[0], 2, "两个点都该数上 / both dots should be counted");
        assert_eq!(moved[0], 2, "两个点都动了 / both dots moved");
    }

    #[test]
    fn 限定区域外的不数_dots_outside_the_area_are_skipped() {
        let first = vec![(5.0, 5.0), (95.0, 95.0)];
        let second = vec![(5.0, 5.0), (95.0, 95.0)];
        let pictures = vec![first, second];

        // 只数左上角那格。
        // Only count the top-left square.
        let (total, _) = movement_grid(&pictures, 50.0, 50.0, 2, 2, Some((0, 1, 0, 1)));

        assert_eq!(total[0], 1, "只该数到左上那个点 / only the top-left dot should count");
        assert_eq!(total[3], 0, "右下那个不该数 / the bottom-right one should not");
    }

    #[test]
    fn 空画面不会崩_empty_pictures_do_not_crash() {
        let pictures: Vec<Vec<(f64, f64)>> = vec![vec![], vec![]];
        let (total, moved) = movement_grid(&pictures, 10.0, 10.0, 2, 2, None);
        assert_eq!(total.iter().sum::<i64>(), 0);
        assert_eq!(moved.iter().sum::<i64>(), 0);
    }
}
