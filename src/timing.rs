//! 记时间：看每一步花了多久
//! Timing: see how long each step took
//!
//! 一次绕过要走好几步（做验证码、提交、查钥匙……），想知道慢在哪就得分开记。
//! 这里做的就是一个很简单的秒表：按名字累加时间，最后能列出清单。
//!
//! One bypass goes through several steps (picture puzzle, submit, check for the
//! key, and so on), and to know where the time goes you have to record them
//! separately. This is a plain stopwatch: it adds up time under a name and can
//! print a list at the end.

use std::collections::BTreeMap;
use std::time::Instant;

/// 秒表。按步骤名字累加耗时。
/// Stopwatch. Adds up time spent, grouped by step name.
#[derive(Default)]
pub struct Timer {
    /// 步骤名字 -> 累计秒数。用 BTreeMap 是为了每次列出来顺序固定。
    /// Step name -> total seconds. BTreeMap keeps the listing order steady.
    steps: BTreeMap<String, f64>,
    /// 当前正在记的那一步，和它的起点。
    /// The step being timed right now, and when it started.
    current: Option<(String, Instant)>,
    /// 如果这次是因为"链接本身没用"而停下，把原因记在这。
    /// If we stopped because the link itself is no good, the reason goes here.
    pub invalid_reason: Option<String>,
}

impl Timer {
    /// 新建一个空秒表。
    /// Make a new, empty stopwatch.
    pub fn new() -> Self {
        Self::default()
    }

    /// 开始记某一步。
    ///
    /// 上一步还没停就先把它停掉，免得忘了停导致时间算不清。
    ///
    /// Start timing a step.
    ///
    /// If a previous step is still running it gets stopped first, so a forgotten
    /// stop cannot muddle the numbers.
    pub fn start(&mut self, name: &str) {
        if self.current.is_some() {
            self.stop();
        }
        self.current = Some((name.to_string(), Instant::now()));
    }

    /// 停掉当前这一步，把耗时加进去。
    /// Stop the current step and add its time in.
    pub fn stop(&mut self) {
        if let Some((name, started)) = self.current.take() {
            *self.steps.entry(name).or_insert(0.0) += started.elapsed().as_secs_f64();
        }
    }

    /// 直接加一笔时间，不用开始和停止。
    /// Add a time directly, without starting and stopping.
    pub fn add(&mut self, name: &str, seconds: f64) {
        *self.steps.entry(name.to_string()).or_insert(0.0) += seconds;
    }

    /// 全部步骤加起来多少秒。
    /// Total seconds across all steps.
    pub fn total(&self) -> f64 {
        self.steps.values().sum()
    }

    /// 有没有记到任何东西。
    /// Whether anything was recorded at all.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// 遍历所有步骤，用来把多次的结果合并起来。
    /// Walk through all steps, used to merge several runs together.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &f64)> {
        self.steps.iter()
    }

    /// 排成一份好读的清单，耗时多的排前面，带百分比。
    /// Lay it out as a readable list, slowest first, with percentages.
    pub fn summary(&self) -> String {
        let total = self.total();
        let mut rows: Vec<(&String, &f64)> = self.steps.iter().collect();

        // 按耗时从大到小排，一眼看到最慢的。
        // Sort longest first, so the slowest jumps out.
        rows.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

        rows.iter()
            .map(|(name, secs)| {
                let share = if total > 0.0 { **secs / total * 100.0 } else { 0.0 };
                format!("    {}: {:.1}s ({:.0}%)", name, secs, share)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// 步骤名字，写成常量避免各处拼错。
/// Step names as constants, so they cannot be misspelled in different places.
pub mod steps {
    /// 做验证码。
    /// Working out the picture puzzle.
    pub const CAPTCHA: &str = "captcha";
    /// 问服务器这个链接的情况。
    /// Asking the server about this link.
    pub const META: &str = "meta";
    /// 提交，往前推进一关。
    /// Submitting, to move one checkpoint forward.
    pub const STEP: &str = "step";
    /// 查钥匙好了没。
    /// Checking whether the key is ready.
    pub const POLL: &str = "poll";
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn 记了才有时间_records_only_what_ran() {
        let mut t = Timer::new();
        assert!(t.is_empty(), "刚建好应该是空的 / a fresh timer should be empty");

        t.start("甲 / first");
        sleep(Duration::from_millis(20));
        t.stop();

        assert!(!t.is_empty());
        assert!(t.total() >= 0.015, "应该记到大约 20 毫秒 / should have recorded roughly 20ms");
    }

    #[test]
    fn 同名累加_same_name_adds_up() {
        let mut t = Timer::new();
        t.add("甲 / first", 1.0);
        t.add("甲 / first", 2.0);
        assert_eq!(t.total(), 3.0, "同一个名字应该累加 / the same name should add up");
    }

    #[test]
    fn 忘了停也不会乱_forgetting_to_stop_is_safe() {
        let mut t = Timer::new();
        t.start("甲 / first");
        sleep(Duration::from_millis(10));

        // 没停就直接开下一步，上一步应该被自动停掉。
        // Starting the next step without stopping should stop the previous one.
        t.start("乙 / second");
        sleep(Duration::from_millis(10));
        t.stop();

        assert_eq!(t.iter().count(), 2, "两步都该记上 / both steps should be recorded");
    }

    #[test]
    fn 清单按耗时排序_summary_is_sorted_by_time() {
        let mut t = Timer::new();
        t.add("快的 / quick", 0.1);
        t.add("慢的 / slow", 5.0);

        let text = t.summary();
        let slow_at = text.find("慢的").expect("清单里应该有慢的那行 / summary should list the slow one");
        let quick_at = text.find("快的").expect("清单里应该有快的那行 / summary should list the quick one");
        assert!(slow_at < quick_at, "慢的应该排在前面 / the slow one should come first");
    }
}
