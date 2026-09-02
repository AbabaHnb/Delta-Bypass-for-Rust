//! 把挨在一起的暗点圈成一块
//! Group touching dark dots into patches
//!
//! 一张图上哪些点算"同一个东西"？答案是：上下左右挨着的算一块。这个文件就干这
//! 件事，顺便算出每块的中心点、大小和外框。
//!
//! 用的是"发现挨着就记下来，最后统一归并"的办法，比反复扫图快。
//!
//! Which dots on a picture count as "the same thing"? Answer: ones touching left,
//! right, above or below. That is what this file does, and it also works out each
//! patch's middle, size and outer box.
//!
//! It notes down what touches what and merges at the end, which is faster than
//! scanning the picture over and over.

use super::Grey;

/// 一块的外框：最左、最右、最上、最下。
/// A patch's outer box: leftmost, rightmost, topmost, bottommost.
pub type Box2D = (usize, usize, usize, usize);

/// 一张图上圈出来的所有块。
///
/// 编号从 1 开始，0 表示"这个点不属于任何块"。
///
/// All the patches found on one picture.
///
/// Numbering starts at 1; 0 means "this dot belongs to no patch".
pub struct Patches {
    /// 每个位置属于哪一块。
    /// Which patch each spot belongs to.
    pub labels: Vec<i32>,
    /// 发出过的最大编号。
    ///
    /// **注意这不等于实际有几块。** 扫图的时候两条线可能先各拿一个编号，后来发现
    /// 它们其实连着（比如 U 形），就归并成一个 —— 被归并掉的那个编号还占着数，只是
    /// 它的 `size` 是 0。
    ///
    /// 所以遍历时一律带上 `size` 判断，别拿这个数当块数。要真实块数用
    /// [`Patches::real_count`]。
    ///
    /// The highest number issued.
    ///
    /// **Note this is not how many patches there actually are.** While scanning, two
    /// lines may each take a number before turning out to be connected (a U shape, for
    /// instance), at which point they merge — and the number that lost still counts
    /// here, just with a `size` of 0.
    ///
    /// So always check `size` while walking through, and do not treat this as a patch
    /// count. For the real count use [`Patches::real_count`].
    pub count: usize,
    /// 每块的中心点。
    /// The middle of each patch.
    pub middle: Vec<(f64, f64)>,
    /// 每块占了多少个点。归并掉的编号是 0。
    /// How many dots each patch covers. Merged-away numbers are 0.
    pub size: Vec<usize>,
    /// 每块的外框。
    /// Each patch's outer box.
    pub boxes: Vec<Box2D>,
}

impl Patches {
    /// 真实有几块（不算归并掉的空编号）。
    /// How many patches there really are, not counting merged-away numbers.
    pub fn real_count(&self) -> usize {
        (1..=self.count).filter(|&n| self.size[n] > 0).count()
    }

    /// 一块的外框面积。
    ///
    /// 拿它跟实际点数一比，就能看出这块"实不实":一个圆点的点数接近外框面积，
    /// 一条细线的点数远小于外框面积。
    ///
    /// The area of a patch's outer box.
    ///
    /// Compare it with the actual dot count and you can tell how solid a patch is:
    /// a round blob's count is close to its box area, a thin line's count is far
    /// below it.
    #[inline]
    pub fn box_area(&self, which: usize) -> f64 {
        if self.size[which] == 0 {
            return 0.0;
        }
        let (left, right, top, bottom) = self.boxes[which];
        ((right - left + 1) * (bottom - top + 1)) as f64
    }
}

/// 顺着记录往上找，找到这一串最终归到哪个编号。
///
/// 顺手把中间经过的编号也改成指向更上面，下次找就更快。
///
/// Follow the notes upwards to find which number this chain ends at.
///
/// It also points the ones it passes further up along the way, so next time is
/// faster.
#[inline]
fn root_of(notes: &mut [u32], mut which: u32) -> u32 {
    while notes[which as usize] != which {
        notes[which as usize] = notes[notes[which as usize] as usize];
        which = notes[which as usize];
    }
    which
}

/// 把选中的点圈成块。
///
/// `chosen` 里 true 的位置参与圈块。从上到下、从左到右扫一遍：如果上面和左边都
/// 没块，就开一个新块；只有一边有，就归到那一边；两边都有且编号不同，就记一笔
/// "这两个其实是一块"，最后统一归并。
///
/// Group the chosen dots into patches.
///
/// Spots marked true in `chosen` take part. Scan top to bottom, left to right: if
/// neither above nor left has a patch, start a new one; if only one side does, join
/// it; if both do with different numbers, note that "these two are really one" and
/// merge at the end.
pub fn find(chosen: &[bool], width: usize, height: usize) -> Patches {
    let total = width * height;
    let mut labels = vec![0i32; total];

    // notes[n] 指向 n 归到哪。一开始每个都指向自己。
    // notes[n] points at where n belongs. To start with, each points at itself.
    let mut notes: Vec<u32> = vec![0];
    let mut next_number: u32 = 0;

    for y in 0..height {
        for x in 0..width {
            let here = y * width + x;
            if !chosen[here] {
                continue;
            }

            let above = if y > 0 { labels[here - width] } else { 0 };
            let left = if x > 0 { labels[here - 1] } else { 0 };

            let mine: u32 = if above == 0 && left == 0 {
                // 两边都没有，开个新块。
                // Neither side has one, so start a new patch.
                next_number += 1;
                notes.push(next_number);
                next_number
            } else if above == 0 {
                left as u32
            } else if left == 0 {
                above as u32
            } else {
                // 两边都有，记一笔说它们是一块，小编号当代表。
                // Both sides have one; note that they are one patch, with the
                // smaller number as the representative.
                let a = root_of(&mut notes, above as u32);
                let b = root_of(&mut notes, left as u32);
                let (small, large) = if a < b { (a, b) } else { (b, a) };
                notes[large as usize] = small;
                small
            };

            labels[here] = mine as i32;
        }
    }

    let count = next_number as usize;

    let mut middle = vec![(0.0f64, 0.0f64); count + 1];
    let mut size = vec![0usize; count + 1];
    let mut boxes = vec![(usize::MAX, 0usize, usize::MAX, 0usize); count + 1];
    let mut sum_x = vec![0.0f64; count + 1];
    let mut sum_y = vec![0.0f64; count + 1];

    // 再扫一遍：把编号统一成代表编号，同时统计每块的中心、大小和外框。
    // Second pass: settle every number onto its representative, and while we are
    // there tally each patch's middle, size and box.
    for y in 0..height {
        for x in 0..width {
            let here = y * width + x;
            if labels[here] == 0 {
                continue;
            }

            let settled = root_of(&mut notes, labels[here] as u32);
            labels[here] = settled as i32;
            let n = settled as usize;

            size[n] += 1;
            sum_x[n] += x as f64;
            sum_y[n] += y as f64;

            let b = &mut boxes[n];
            if x < b.0 {
                b.0 = x;
            }
            if x > b.1 {
                b.1 = x;
            }
            if y < b.2 {
                b.2 = y;
            }
            if y > b.3 {
                b.3 = y;
            }
        }
    }

    for n in 1..=count {
        if size[n] > 0 {
            middle[n] = (sum_x[n] / size[n] as f64, sum_y[n] / size[n] as f64);
        } else {
            // 归并之后空掉的编号，外框清成 0，别留着 usize::MAX。
            // Numbers left empty after merging: clear the box to 0 rather than
            // leaving usize::MAX in there.
            boxes[n] = (0, 0, 0, 0);
        }
    }

    Patches { labels, count, middle, size, boxes }
}

/// 只要够大的块的中心点。
///
/// 太小的块基本是噪点，直接不要。
///
/// Just the middles of patches that are big enough.
///
/// Tiny patches are mostly speckle, so they are dropped.
pub fn middles(chosen: &[bool], width: usize, height: usize, least_size: usize) -> Vec<(f64, f64)> {
    let found = find(chosen, width, height);
    (1..=found.count)
        .filter(|&n| found.size[n] >= least_size)
        .map(|n| found.middle[n])
        .collect()
}

// ---------------------------------------------------------------------------
// 挑出暗点 / Picking out dark dots
// ---------------------------------------------------------------------------

/// 按红绿蓝三个通道都够暗来挑。
/// Pick dots where red, green and blue are all dark enough.
pub fn dark_by_colour(pixels: &[u8], width: usize, height: usize) -> Vec<bool> {
    let mut out = Vec::with_capacity(width * height);
    for i in 0..width * height {
        out.push(pixels[i * 4] < 150 && pixels[i * 4 + 1] < 150 && pixels[i * 4 + 2] < 150);
    }
    out
}

/// 按明暗值低于门槛来挑。
/// Pick dots whose brightness is below a cut-off.
pub fn dark_by_brightness(grey: &Grey, cutoff: f32) -> Vec<bool> {
    grey.values.iter().map(|&v| v < cutoff).collect()
}

/// 一张画面上所有暗块的中心点。
///
/// `by_colour` 为真就按三通道挑，否则按明暗值挑。
///
/// The middles of every dark patch on one picture.
///
/// With `by_colour` true it picks by the three colour channels, otherwise by
/// brightness.
pub fn dark_middles(
    pixels: &[u8],
    width: usize,
    height: usize,
    by_colour: bool,
) -> Vec<(f64, f64)> {
    let chosen = if by_colour {
        dark_by_colour(pixels, width, height)
    } else {
        let grey = super::to_grey(pixels, width, height);
        dark_by_brightness(&grey, 170.0)
    };
    middles(&chosen, width, height, 3)
}

/// 列出所有暗点的坐标。
///
/// 跟上面不同：这里要的是每一个点，不是每一块的中心。
///
/// List the positions of every dark dot.
///
/// Different from above: this wants each individual dot, not each patch's middle.
pub fn dark_dots(grey: &Grey, cutoff: f32) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for y in 0..grey.height {
        let row = y * grey.width;
        for x in 0..grey.width {
            if grey.values[row + x] < cutoff {
                out.push((x as f64, y as f64));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 挨着的算一块_touching_dots_form_one_patch() {
        // 4 宽 3 高。左上角三个点连着，右边一个点单独。
        // 4 wide, 3 tall. Three connected dots top-left, one on its own to the right.
        let w = 4;
        let h = 3;
        let mut chosen = vec![false; w * h];
        chosen[0] = true;         // (0,0)
        chosen[1] = true;         // (1,0)
        chosen[w] = true;         // (0,1)
        chosen[w + 3] = true;     // (3,1)

        let found = find(&chosen, w, h);

        assert_eq!(found.real_count(), 2, "应该是两块 / should be two patches");
        assert_eq!(found.size[1], 3, "第一块三个点 / first patch has three dots");
        assert_eq!(found.size[2], 1, "第二块一个点 / second patch has one dot");
        assert_eq!(found.boxes[1], (0, 1, 0, 1), "第一块外框 / first patch box");
    }

    #[test]
    fn 斜着不算一块_diagonal_dots_stay_apart() {
        // 只有斜对角挨着，按规矩不算一块。
        // Only touching corner to corner, which by our rule is not one patch.
        let w = 3;
        let h = 2;
        let mut chosen = vec![false; w * h];
        chosen[0] = true;         // (0,0)
        chosen[w + 1] = true;     // (1,1)

        let found = find(&chosen, w, h);
        assert_eq!(found.real_count(), 2, "斜着的应该算两块 / diagonal should be two patches");
    }

    #[test]
    fn 需要归并的形状也对_shapes_needing_a_merge_are_right() {
        // U 形：左右两条竖线，底下连起来。扫的时候会先当成两块，最后归并成一块。
        // 归并掉的编号还占着号，所以要看 real_count 而不是 count。
        //
        // A U shape: two uprights joined at the bottom. The scan sees two patches at
        // first and merges them into one at the end. The number that lost still counts,
        // so check real_count rather than count.
        let w = 3;
        let h = 3;
        let mut chosen = vec![false; w * h];
        for y in 0..3 {
            chosen[y * w] = true;         // 左边一竖 / left upright
            chosen[y * w + 2] = true;     // 右边一竖 / right upright
        }
        chosen[2 * w + 1] = true;         // 底下连接 / bottom join

        let found = find(&chosen, w, h);
        assert_eq!(found.real_count(), 1, "U 形应该是一块 / a U shape should be one patch");

        // 那一块占 7 个点。找出唯一非空的编号来看。
        // That patch covers 7 dots. Find the one non-empty number and check it.
        let only = (1..=found.count)
            .find(|&n| found.size[n] > 0)
            .expect("应该有一块 / there should be one patch");
        assert_eq!(found.size[only], 7);

        // 拿中心点的那个接口也应该只给出一块。
        // The middles interface should also give just one patch.
        assert_eq!(middles(&chosen, w, h, 1).len(), 1);
    }

    #[test]
    fn 中心点算得对_middle_is_correct() {
        // 一条横线，中心应该在正中间。
        // A horizontal line; its middle should sit halfway along.
        let w = 5;
        let h = 1;
        let mut chosen = vec![false; w * h];
        chosen.fill(true);

        let found = find(&chosen, w, h);
        assert_eq!(found.real_count(), 1);
        assert!((found.middle[1].0 - 2.0).abs() < 1e-9, "横向中心应为 2 / middle across should be 2");
        assert!((found.middle[1].1 - 0.0).abs() < 1e-9);
    }

    #[test]
    fn 太小的块会被扔掉_tiny_patches_are_dropped() {
        let w = 4;
        let h = 2;
        let mut chosen = vec![false; w * h];
        // 一块三个点，一块一个点。
        // One patch of three dots, one of a single dot.
        chosen[0] = true;
        chosen[1] = true;
        chosen[2] = true;
        chosen[w + 3] = true;

        let kept = middles(&chosen, w, h, 3);
        assert_eq!(kept.len(), 1, "只该留下够大的那块 / only the big enough patch should stay");
    }

    #[test]
    fn 全空不会崩_all_empty_does_not_crash() {
        let empty = vec![false; 12];
        let found = find(&empty, 4, 3);
        assert_eq!(found.real_count(), 0);
        assert!(middles(&empty, 4, 3, 1).is_empty());
    }

    #[test]
    fn 外框面积能区分形状_box_area_tells_shapes_apart() {
        // 实心方块：点数接近外框面积。
        // A solid square: dot count close to its box area.
        let w = 4;
        let h = 4;
        let mut solid = vec![false; w * h];
        for y in 1..3 {
            for x in 1..3 {
                solid[y * w + x] = true;
            }
        }
        let found = find(&solid, w, h);
        assert_eq!(found.size[1], 4);
        assert_eq!(found.box_area(1), 4.0, "实心方块点数应等于外框面积 / a solid square fills its box");
    }
}
