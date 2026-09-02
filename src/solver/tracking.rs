//! 追踪：看每个形状在动画里怎么动
//! Tracking: watch how each shape moves through the animation
//!
//! 两种看法：
//!
//! 一种是"定个明暗门槛，比这暗的算形状"。简单直接，但背景稍微花一点就容易把不
//! 相干的东西也算进来。
//!
//! 另一种是"先算出不动的背景，再看哪儿跟背景不一样"。会动的东西自然就露出来了，
//! 抗花背景，是主要用的办法。
//!
//! 不管哪种，拿到每张画面上的形状之后都要"串起来"：这一张的这个形状，在下一张里
//! 是哪个？串成一条轨迹，就能算出它绕着转了多少、转得稳不稳。
//!
//! Two ways of looking:
//!
//! One is "set a brightness cut-off; anything darker counts as a shape". Simple and
//! direct, but a slightly busy background easily drags in things that do not belong.
//!
//! The other is "work out the background that does not move, then look at what
//! differs from it". Moving things stand out by themselves, it copes with busy
//! backgrounds, and it is the one mainly used.
//!
//! Either way, once we have the shapes on each picture they need joining up: this
//! shape here, which one is it in the next picture? Joined into a trail, we can work
//! out how far it went round and how steadily.

use rayon::prelude::*;

use crate::image::{self, Frames, Grey, NearestFinder};

/// 一个形状走完全程之后的总结。
/// A summary of one shape after its whole journey.
#[derive(Clone, Copy)]
pub struct Track {
    /// 它在第一张画面上的位置。这才是要点的地方。
    /// Where it was on the first picture. This is the spot we would click.
    pub x: f64,
    pub y: f64,
    /// 每一步平均转了多少（弧度）。
    /// How far it turned per step on average, in radians.
    pub per_step: f64,
    /// 有多可信。转得稳、轨迹圆的分高。
    /// How trustworthy it looks. Steady turning on a round trail scores high.
    pub confidence: f64,
    /// 方向有多一致。1 表示一直往一个方向转，0 表示来回晃。
    /// How consistent the direction is. 1 means always one way, 0 means it wobbles.
    pub steadiness: f64,
    /// 轨迹离正圆差多少。越小越圆。
    /// How far the trail is from a true circle. Smaller is rounder.
    pub roundness_error: f64,
    /// 配出来的圆有多大。
    /// How big the fitted circle is.
    pub radius: f64,
    /// 全程一共转了多少（弧度）。正负表示两个方向。
    /// How far it turned in total, in radians. The sign shows which way.
    pub total_turn: f64,
    /// 轨迹够不够圆，能不能当成"绕圈转"看。
    /// Whether the trail is round enough to treat as going round in a circle.
    pub is_circular: bool,
    /// 它在第一张画面上占多少个点。
    /// How many dots it covered on the first picture.
    pub area: f64,
    /// 有多"实"。1 是实心块，接近 0 是细线条。
    /// How solid it is. 1 is a filled blob, near 0 is a thin line.
    pub solidity: f64,
    /// 一共跟到了几张画面。
    /// How many pictures we managed to follow it through.
    pub seen_in: usize,
    /// 是哪个明暗门槛下找到的。背景差分法记 0。
    /// Which brightness cut-off found it. The background method records 0.
    pub cutoff: f64,
}

/// 一张画面上找到的形状：中心点和大小。
/// The shapes found on one picture: middles and sizes.
struct Found {
    middles: Vec<(f64, f64)>,
    sizes: Vec<usize>,
}

/// 一个选上的起点，连它跟出来的轨迹。
///
/// 依次是：起始画面序号、起点横向、起点纵向、起点占多少个点、轨迹。
///
/// One accepted starting point, together with the trail it produced.
///
/// In order: which picture it started on, its position across and down, how many dots
/// it covered, and the trail.
type Accepted = (usize, f64, f64, f64, Vec<(usize, (f64, f64))>);

/// 一条轨迹算出来的各项数字。
/// The numbers worked out from one trail.
struct Motion {
    centre_x: f64,
    centre_y: f64,
    radius: f64,
    is_circular: bool,
    angles: Vec<f64>,
    total_turn: f64,
    middle_step: f64,
    typical_step: f64,
    typical_radius: f64,
    roundness_error: f64,
    steadiness: f64,
    confidence: f64,
}

/// 从一条轨迹算出它怎么转的。
///
/// 步骤：先给这串点配个圆当转动中心（配不出圆就用点的平均位置）；然后算每个点相
/// 对中心的角度，理顺之后看总共转了多少；再看每步转的量稳不稳、点离圆有多远。
///
/// Work out how a trail turned.
///
/// Steps: fit a circle to the dots to act as the centre of rotation (if no circle
/// fits, use the average position); work out each dot's angle from that centre,
/// straighten them out and see how far it went in total; then check how steady the
/// per-step turning was and how far the dots sit from the circle.
fn measure(dots: &[(f64, f64)]) -> Motion {
    let n = dots.len() as f64;
    let average_x = dots.iter().map(|d| d.0).sum::<f64>() / n;
    let average_y = dots.iter().map(|d| d.1).sum::<f64>() / n;

    let (fit_x, fit_y, radius) = image::fit_circle(dots);

    // 半径太小或太大都说明配得不像圆，那就用平均位置当中心。
    // A radius that is too small or too large means the fit is not circle-like, so
    // fall back to the average position as the centre.
    let (centre_x, centre_y, is_circular) = if radius > 5.0 && radius < 250.0 {
        (fit_x, fit_y, true)
    } else {
        (average_x, average_y, false)
    };

    let raw_angles: Vec<f64> = dots
        .iter()
        .map(|d| (d.1 - centre_y).atan2(d.0 - centre_x))
        .collect();
    let angles = image::unwrap_angles(&raw_angles);

    let steps: Vec<f64> = angles.windows(2).map(|w| w[1] - w[0]).collect();
    let total_turn = angles[angles.len() - 1] - angles[0];
    let middle_step = image::middle_value(&steps);

    // 找出"大多数步子"的典型值：先看每步偏离中间值多少，再把偏得太离谱的扔掉。
    // 这样个别抖动不会影响判断。
    // Find the typical step among most of them: see how far each strays from the
    // middle, then drop the wild ones. That way the odd jitter does not sway things.
    let strays: Vec<f64> = steps.iter().map(|s| (s - middle_step).abs()).collect();
    let typical_stray = image::middle_value(&strays) + 1e-6;
    let sensible: Vec<f64> = steps
        .iter()
        .copied()
        .filter(|s| (s - middle_step).abs() < 3.5 * typical_stray)
        .collect();
    let typical_step = if sensible.len() >= 3 {
        image::middle_value(&sensible)
    } else {
        middle_step
    };

    // 各点到中心的距离。都差不多说明轨迹是个圆。
    // Each dot's distance to the centre. All similar means the trail is a circle.
    let distances: Vec<f64> = dots
        .iter()
        .map(|d| ((d.0 - centre_x).powi(2) + (d.1 - centre_y).powi(2)).sqrt())
        .collect();
    let typical_radius = image::middle_value(&distances);
    let radius_strays: Vec<f64> = distances
        .iter()
        .map(|d| (d - typical_radius).abs())
        .collect();
    let roundness_error = image::middle_value(&radius_strays) / (typical_radius + 1e-6);

    // 方向一致度：每步的方向取正负号求平均。一直同向就接近 1。
    // Direction consistency: average the sign of each step. Always the same way
    // gives close to 1.
    let steadiness =
        steps.iter().map(|s| image::direction(*s)).sum::<f64>().abs() / steps.len() as f64;

    // 可信度：方向越一致越高，轨迹越不圆越低。
    // Trustworthiness: higher the more consistent the direction, lower the less
    // round the trail.
    let confidence = steadiness / (1.0 + 10.0 * roundness_error);

    Motion {
        centre_x,
        centre_y,
        radius,
        is_circular,
        angles,
        total_turn,
        middle_step,
        typical_step,
        typical_radius,
        roundness_error,
        steadiness,
        confidence,
    }
}

/// 把一个形状从起始画面一路跟到最后。
///
/// 每张都找离上次位置最近的那个形状，近到一定程度就认为是同一个。`allow_gaps` 为
/// 真时，某张没找到就跳过继续（背景差分法用）；为假时也是跳过，但会保留在原位继续
/// 找（明暗门槛法用）。
///
/// Follow one shape from its starting picture to the end.
///
/// In each picture we find the shape nearest to where it was, and if it is close
/// enough we call it the same one. With `allow_gaps` true, a picture where nothing
/// matches is skipped and we carry on (used by the background method); with it
/// false we also carry on but stay put and keep looking (used by the cut-off
/// method).
fn follow(
    per_picture: &[Vec<(f64, f64)>],
    from: usize,
    start: (f64, f64),
    close_enough: f64,
) -> Vec<(usize, (f64, f64))> {
    let mut here = start;
    let mut trail = vec![(from, here)];

    // 从起始画面的下一张开始往后跟。带上序号，因为轨迹里要记"这是第几张"。
    // Follow along from the picture after the starting one. The number comes too, since
    // the trail records which picture each point came from.
    for (which, candidates) in per_picture.iter().enumerate().skip(from + 1) {
        if candidates.is_empty() {
            continue;
        }

        let finder = NearestFinder::build(candidates);
        let (index, distance) = finder.nearest(here);

        if distance < close_enough {
            here = candidates[index];
            trail.push((which, here));
        }
    }

    trail
}

/// 按明暗门槛找形状并追踪。
///
/// 比 `cutoff` 暗的点圈成块，太小的不要。以第一张画面上的块为起点，一路跟下去。
/// 靠边的不要 —— 边上的形状容易被裁掉一半，位置算不准。
///
/// Find shapes by brightness cut-off and track them.
///
/// Dots darker than `cutoff` get grouped into patches, and small ones are dropped.
/// Patches on the first picture become the starting points, and we follow each one
/// along. Ones near the edge are skipped — edge shapes are easily cut in half, so
/// their position is unreliable.
pub fn track_by_cutoff(
    frames: &Frames,
    cutoff: f32,
    least_size: usize,
    close_enough: f64,
    greys: &[Grey],
) -> Vec<Track> {
    let width = frames.width;
    let height = frames.height;

    // 第一张要留完整的圈块结果，因为起点要看外框算"实不实"。其余画面只要中心点。
    // The first picture keeps its full patch results, since the starting points need
    // the outer box to judge solidity. The rest only need middles.
    let first_chosen = image::dark_by_brightness(&greys[0], cutoff);

    let (first_patches, rest) = rayon::join(
        || image::find_patches(&first_chosen, width, height),
        || {
            greys[1..]
                .par_iter()
                .map(|g| {
                    let chosen = image::dark_by_brightness(g, cutoff);
                    let found = image::find_patches(&chosen, width, height);
                    let mut middles = Vec::new();
                    let mut sizes = Vec::new();
                    for n in 1..=found.count {
                        if found.size[n] >= least_size {
                            middles.push(found.middle[n]);
                            sizes.push(found.size[n]);
                        }
                    }
                    Found { middles, sizes }
                })
                .collect::<Vec<Found>>()
        },
    );

    let mut per_picture: Vec<Vec<(f64, f64)>> = Vec::with_capacity(frames.count());
    per_picture.push(
        (1..=first_patches.count)
            .filter(|&n| first_patches.size[n] >= least_size)
            .map(|n| first_patches.middle[n])
            .collect(),
    );
    per_picture.extend(rest.into_iter().map(|f| f.middles));

    // 挑起点：够大、不靠边。
    // Choose starting points: big enough, not near the edge.
    let starts: Vec<usize> = (1..=first_patches.count)
        .filter(|&n| first_patches.size[n] >= least_size)
        .filter(|&n| {
            let (x, y) = first_patches.middle[n];
            x >= 15.0 && x <= (width - 15) as f64 && y >= 15.0 && y <= (height - 15) as f64
        })
        .collect();

    // 每个起点互不相干，分给多个核同时跟。
    // Starting points do not affect each other, so split them across cores.
    starts
        .into_par_iter()
        .filter_map(|n| {
            let (x, y) = first_patches.middle[n];
            let area = first_patches.size[n] as f64;
            let solidity = area / first_patches.box_area(n).max(1.0);

            let trail = follow(&per_picture, 0, (x, y), close_enough);
            let dots: Vec<(f64, f64)> = trail.iter().map(|t| t.1).collect();

            // 跟到的画面太少，算不出可靠的转动，扔掉。
            // Too few pictures to work out reliable motion, so drop it.
            if dots.len() < 6 {
                return None;
            }

            let m = measure(&dots);

            Some(Track {
                x,
                y,
                per_step: m.typical_step,
                confidence: m.confidence,
                steadiness: m.steadiness,
                roundness_error: m.roundness_error,
                radius: m.radius,
                total_turn: m.total_turn,
                is_circular: m.is_circular,
                area,
                solidity,
                seen_in: dots.len(),
                cutoff: cutoff as f64,
            })
        })
        .collect()
}

/// 先算不动的背景，再追踪跟背景不一样的地方。
///
/// 这是主要用的办法，比定门槛抗干扰。`difference` 是"跟背景差多少才算动了"。
///
/// 起点可以从前几张里任选（有的形状第一张时被挡住了）。如果起点不在第一张，最后
/// 会把位置往回推算到第一张 —— 因为要点的是第一张上的位置。
///
/// Work out the background that does not move, then track what differs from it.
///
/// This is the one mainly used; it copes with clutter better than a fixed cut-off.
/// `difference` is how far from the background counts as having moved.
///
/// Starting points may come from any of the first few pictures, since some shapes
/// are hidden at the very start. If a start is not on the first picture, its
/// position is worked back to the first one at the end — because the first picture
/// is where we would click.
pub fn track_by_background(
    frames: &Frames,
    difference: f32,
    least_size: usize,
    close_enough: f64,
    greys: &[Grey],
) -> Vec<Track> {
    let width = frames.width;
    let height = frames.height;
    let count = greys.len();

    let background = image::background(greys);

    // 每张画面挑出"比背景暗了不少"的点，再圈成块。分给多个核做。
    // For each picture, pick the dots noticeably darker than the background and
    // group them. Split across cores.
    let per_frame: Vec<Found> = greys
        .par_iter()
        .map(|g| {
            // 逐点比对：比背景暗了不少的就选上。
            // Dot by dot: pick the ones noticeably darker than the background.
            let chosen: Vec<bool> = background
                .values
                .iter()
                .zip(g.values.iter())
                .map(|(back, now)| (back - now) > difference)
                .collect();

            let found = image::find_patches(&chosen, width, height);
            let mut middles = Vec::new();
            let mut sizes = Vec::new();
            for n in 1..=found.count {
                if found.size[n] >= least_size {
                    middles.push(found.middle[n]);
                    sizes.push(found.size[n]);
                }
            }
            Found { middles, sizes }
        })
        .collect();

    let per_picture: Vec<Vec<(f64, f64)>> =
        per_frame.iter().map(|f| f.middles.clone()).collect();

    // 挑起点这一步必须按顺序做，因为"离已选起点太近的就不再选"，顺序一换结果就变。
    // 注意：只有真正跟出足够长轨迹的才算占位，否则一个跟不动的候选会把旁边好的挤掉。
    //
    // Choosing starting points has to happen in order, because "skip anything too
    // close to one already chosen" depends on the order. Note: only ones that
    // actually produce a long enough trail take up a slot, otherwise a candidate
    // that cannot be followed would crowd out a good neighbour.
    let mut accepted: Vec<Accepted> = Vec::new();
    let mut taken: Vec<(f64, f64)> = Vec::new();

    for from in 0..count.min(6) {
        if per_picture[from].is_empty() {
            continue;
        }

        for i in 0..per_picture[from].len() {
            let (x, y) = per_picture[from][i];

            // 靠边的不要。
            // Skip ones near the edge.
            if x < 15.0 || x > (width - 15) as f64 || y < 15.0 || y > (height - 15) as f64 {
                continue;
            }

            // 离已选的太近，当成同一个东西，不重复选。
            // Too close to one already chosen; treat it as the same thing and do
            // not choose it twice.
            if taken
                .iter()
                .any(|t| ((x - t.0).powi(2) + (y - t.1).powi(2)).sqrt() < 25.0)
            {
                continue;
            }

            let trail = follow(&per_picture, from, (x, y), close_enough);
            if trail.len() < 6 {
                continue;
            }

            taken.push((x, y));
            accepted.push((from, x, y, per_frame[from].sizes[i] as f64, trail));
        }
    }

    // 算数字的部分互不相干，可以并行。
    // Working out the numbers is independent per trail, so it can run in parallel.
    accepted
        .into_par_iter()
        .map(|(from, x, y, area, trail)| {
            let picture_numbers: Vec<f64> = trail.iter().map(|t| t.0 as f64).collect();
            let dots: Vec<(f64, f64)> = trail.iter().map(|t| t.1).collect();
            let m = measure(&dots);

            // 起点不在第一张的话，按转动规律往回推算它在第一张的位置。
            // If the start is not on the first picture, work its first-picture
            // position back from how it turned.
            let (mut first_x, mut first_y) = (x, y);
            if from > 0 && m.is_circular && picture_numbers.len() >= 6 {
                let (_, angle_at_zero) = image::fit_line(&picture_numbers, &m.angles);
                let back_x = m.centre_x + m.typical_radius * angle_at_zero.cos();
                let back_y = m.centre_y + m.typical_radius * angle_at_zero.sin();

                // 推算出来的位置得在画面里、不靠边，才采用。
                // Only use the worked-back position if it lands inside the picture
                // and not near the edge.
                if back_x >= 15.0
                    && back_x <= (width - 15) as f64
                    && back_y >= 15.0
                    && back_y <= (height - 15) as f64
                {
                    first_x = back_x;
                    first_y = back_y;
                }
            }

            Track {
                x: first_x,
                y: first_y,
                // 这里用中间值而不是"典型值"，跟原版保持一致。
                // Uses the middle value rather than the typical one, to match the
                // original.
                per_step: m.middle_step,
                confidence: m.confidence,
                steadiness: m.steadiness,
                roundness_error: m.roundness_error,
                radius: m.radius,
                total_turn: m.total_turn,
                is_circular: m.is_circular,
                area,
                // 背景差分法不算"实不实"，统一记 1。
                // The background method does not judge solidity, so it records 1.
                solidity: 1.0,
                seen_in: trail.len(),
                cutoff: 0.0,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一条完美的圆形轨迹，检查各项数字算得对。
    /// Build a perfect circular trail and check the numbers come out right.
    #[test]
    fn 圆形轨迹数字正确_circular_trail_numbers_are_right() {
        let (cx, cy, r) = (100.0f64, 80.0f64, 20.0f64);
        let dots: Vec<(f64, f64)> = (0..24)
            .map(|k| {
                let angle = k as f64 * std::f64::consts::TAU / 24.0;
                (cx + r * angle.cos(), cy + r * angle.sin())
            })
            .collect();

        let m = measure(&dots);

        assert!(m.is_circular, "应该判成圆形 / should be judged circular");
        assert!((m.centre_x - cx).abs() < 0.5, "圆心横向 / centre across");
        assert!((m.centre_y - cy).abs() < 0.5, "圆心纵向 / centre down");
        assert!(m.roundness_error < 0.01, "完美圆的偏差应该很小 / a perfect circle should barely stray");
        assert!(m.steadiness > 0.95, "一直同向转，一致度应该很高 / always one way, so very consistent");
        assert!(m.total_turn > 0.0, "逆时针总转量应为正 / anticlockwise total should be positive");
    }

    #[test]
    fn 反向转的总转量为负_reverse_turn_is_negative() {
        let (cx, cy, r) = (50.0f64, 50.0f64, 15.0f64);
        let dots: Vec<(f64, f64)> = (0..24)
            .map(|k| {
                // 角度递减就是反方向。
                // A decreasing angle means the other direction.
                let angle = -(k as f64) * std::f64::consts::TAU / 24.0;
                (cx + r * angle.cos(), cy + r * angle.sin())
            })
            .collect();

        let m = measure(&dots);
        assert!(m.total_turn < 0.0, "反向总转量应为负 / the other way should be negative");
        assert!(m.steadiness > 0.95);
    }

    #[test]
    fn 乱走的轨迹一致度低_a_wandering_trail_is_inconsistent() {
        // 来回晃，不成圈。
        // Wobbling back and forth, not going round.
        let dots: Vec<(f64, f64)> = (0..20)
            .map(|k| {
                let f = k as f64;
                (50.0 + (f * 1.7).sin() * 8.0, 50.0 + (f * 2.3).cos() * 8.0)
            })
            .collect();

        let m = measure(&dots);
        assert!(
            m.steadiness < 0.9,
            "来回晃的一致度不该高 / wobbling should not look consistent"
        );
    }

    #[test]
    fn 转一整圈总量约等于一圈_a_full_lap_totals_one_turn() {
        let (cx, cy, r) = (60.0f64, 60.0f64, 18.0f64);
        // 走 25 个点刚好绕回起点附近。
        // Twenty-five dots come back around near the start.
        let dots: Vec<(f64, f64)> = (0..25)
            .map(|k| {
                let angle = k as f64 * std::f64::consts::TAU / 24.0;
                (cx + r * angle.cos(), cy + r * angle.sin())
            })
            .collect();

        let m = measure(&dots);
        let one_lap = std::f64::consts::TAU;
        assert!(
            (m.total_turn - one_lap).abs() < 0.2,
            "绕一圈总量应接近一圈 / one lap should total about one turn"
        );
    }
}
