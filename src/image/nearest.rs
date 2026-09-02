//! 找离某个点最近的点
//! Find the nearest dot to a given point
//!
//! 追踪形状的时候要反复问："这一张里的这个点，在下一张里对应哪个？" 答案是"离它
//! 最近的那个"。点多的时候一个个比太慢，所以先按位置把点分成树状结构，找的时候
//! 一大片一大片地跳过去。
//!
//! When tracking shapes we keep asking: "this dot in one picture, which dot does it
//! match in the next?" The answer is "the closest one". Comparing one by one is too
//! slow with many dots, so we sort them into a tree by position and skip whole
//! regions while searching.

/// 树上的一个节点。
/// One node in the tree.
struct Node {
    /// 这个节点存的点。
    /// The dot stored here.
    dot: (f64, f64),
    /// 它在原来那份名单里排第几，找到之后要用序号去取别的信息。
    /// Its place in the original list, needed to look up other details afterwards.
    index: u32,
    /// 这一层按横向还是纵向分：0 是横，1 是纵。
    /// Whether this level splits across or down: 0 is across, 1 is down.
    axis: u8,
    /// 左右两边的子树。
    /// The two branches.
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
}

/// 一堆点，按位置排好，方便快速找最近的。
/// A collection of dots, sorted by position so the nearest can be found quickly.
pub struct NearestFinder {
    root: Option<Box<Node>>,
}

impl NearestFinder {
    /// 把一堆点整理成可以快速查找的结构。
    /// Arrange some dots into a structure that can be searched quickly.
    pub fn build(dots: &[(f64, f64)]) -> Self {
        let mut items: Vec<(u32, (f64, f64))> = dots
            .iter()
            .copied()
            .enumerate()
            .map(|(i, d)| (i as u32, d))
            .collect();

        NearestFinder { root: Self::build_part(&mut items, 0) }
    }

    /// 递归建树。
    ///
    /// 每层换一个方向分：这层按横向，下层按纵向，再下层又按横向。每次取中间那个
    /// 点当分界，左右各一半。
    ///
    /// Build the tree step by step.
    ///
    /// Each level splits a different way: this level across, the next down, then
    /// across again. Each time the middle dot becomes the divider, with half on
    /// each side.
    fn build_part(items: &mut [(u32, (f64, f64))], depth: usize) -> Option<Box<Node>> {
        if items.is_empty() {
            return None;
        }

        let axis = (depth % 2) as u8;
        let middle = items.len() / 2;

        // 只把中间那个挑到正确位置，不用全排序，这样更快。
        // Only put the middle item in its right place rather than sorting
        // everything, which is faster.
        if axis == 0 {
            items.select_nth_unstable_by(middle, |a, b| {
                a.1 .0.partial_cmp(&b.1 .0).unwrap_or(std::cmp::Ordering::Equal)
            });
        } else {
            items.select_nth_unstable_by(middle, |a, b| {
                a.1 .1.partial_cmp(&b.1 .1).unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        let (index, dot) = items[middle];

        Some(Box::new(Node {
            dot,
            index,
            axis,
            left: Self::build_part(&mut items[..middle], depth + 1),
            right: Self::build_part(&mut items[middle + 1..], depth + 1),
        }))
    }

    /// 找离 `target` 最近的点，返回 `(它的序号, 距离)`。
    ///
    /// 名单是空的话返回 `(0, 无穷大)`，调用方一看距离就知道没找到。
    ///
    /// Find the dot nearest to `target`, giving `(its place, distance)`.
    ///
    /// With an empty list it gives `(0, infinity)`, and the caller can tell from
    /// the distance that nothing was found.
    pub fn nearest(&self, target: (f64, f64)) -> (usize, f64) {
        let mut best_index = 0u32;
        // 存平方距离，比开方快；最后再开一次方。
        // Keep the squared distance, which is cheaper; take the square root once
        // at the end.
        let mut best_squared = f64::INFINITY;

        Self::search(self.root.as_deref(), target, &mut best_index, &mut best_squared);

        (best_index as usize, best_squared.sqrt())
    }

    /// 在树里搜。
    ///
    /// 关键在最后那个判断：先往目标所在那一侧找，找完看"另一侧最近也可能有多近"。
    /// 如果连那个下限都比现在找到的还远，另一侧整片都可以跳过。这就是它比逐个比
    /// 较快的原因。
    ///
    /// Search the tree.
    ///
    /// The important part is the last check: look on the side the target is on
    /// first, then ask "how close could the other side possibly be". If even that
    /// best case is further than what we already have, the whole other side gets
    /// skipped. That is why this beats comparing one by one.
    fn search(
        node: Option<&Node>,
        target: (f64, f64),
        best_index: &mut u32,
        best_squared: &mut f64,
    ) {
        let node = match node {
            None => return,
            Some(n) => n,
        };

        let dx = node.dot.0 - target.0;
        let dy = node.dot.1 - target.1;
        let squared = dx * dx + dy * dy;

        if squared < *best_squared {
            *best_squared = squared;
            *best_index = node.index;
        }

        // 目标在分界线的哪一侧。
        // Which side of the divider the target sits on.
        let offset = if node.axis == 0 {
            target.0 - node.dot.0
        } else {
            target.1 - node.dot.1
        };

        let (near_side, far_side) = if offset < 0.0 {
            (node.left.as_deref(), node.right.as_deref())
        } else {
            (node.right.as_deref(), node.left.as_deref())
        };

        Self::search(near_side, target, best_index, best_squared);

        // 另一侧最近也就是到分界线这么远。够远就整片跳过。
        // The other side can at best be as close as the divider. If that is far
        // enough, skip it entirely.
        if offset * offset < *best_squared {
            Self::search(far_side, target, best_index, best_squared);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 能找到最近的_finds_the_closest() {
        let dots = vec![(0.0, 0.0), (10.0, 10.0), (3.0, 4.0)];
        let finder = NearestFinder::build(&dots);

        let (which, distance) = finder.nearest((3.1, 4.1));
        assert_eq!(which, 2, "应该找到 (3,4) 那个 / should find the (3,4) one");
        assert!(distance < 0.2);
    }

    #[test]
    fn 距离算得准_distance_is_accurate() {
        let dots = vec![(0.0, 0.0)];
        let finder = NearestFinder::build(&dots);

        // 3-4-5 直角三角形，距离应该正好是 5。
        // A 3-4-5 triangle, so the distance should be exactly 5.
        let (_, distance) = finder.nearest((3.0, 4.0));
        assert!((distance - 5.0).abs() < 1e-9);
    }

    #[test]
    fn 空名单不会崩_empty_list_does_not_crash() {
        let finder = NearestFinder::build(&[]);
        let (_, distance) = finder.nearest((1.0, 1.0));
        assert!(distance.is_infinite(), "空名单距离应为无穷大 / an empty list should give infinity");
    }

    #[test]
    fn 结果跟逐个比较一致_matches_a_plain_comparison() {
        // 造一批点，跟"老老实实一个个比"的结果对一遍。
        // Make a batch of dots and check against plainly comparing every one.
        let mut dots = Vec::new();
        for i in 0..200 {
            let f = i as f64;
            dots.push((f * 7.3 % 97.0, f * 3.1 % 61.0));
        }
        let finder = NearestFinder::build(&dots);

        for probe in [(10.0, 10.0), (50.0, 30.0), (0.0, 0.0), (96.0, 60.0), (33.3, 21.7)] {
            let (fast_index, fast_distance) = finder.nearest(probe);

            let mut slow_distance = f64::INFINITY;
            for d in dots.iter() {
                let dist = ((d.0 - probe.0).powi(2) + (d.1 - probe.1).powi(2)).sqrt();
                if dist < slow_distance {
                    slow_distance = dist;
                }
            }

            assert!(
                (fast_distance - slow_distance).abs() < 1e-9,
                "快慢两种办法结果应该一样 / the quick and plain ways should agree"
            );
            assert!(fast_index < dots.len());
        }
    }

    #[test]
    fn 点重合也能处理_duplicate_dots_are_fine() {
        let dots = vec![(5.0, 5.0), (5.0, 5.0), (5.0, 5.0)];
        let finder = NearestFinder::build(&dots);
        let (_, distance) = finder.nearest((5.0, 5.0));
        assert!(distance < 1e-9);
    }
}
