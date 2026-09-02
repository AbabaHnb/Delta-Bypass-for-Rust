//! 装成手机浏览器
//! Pretend to be a phone browser
//!
//! 每个请求都要带一段"我是什么浏览器"的说明。上游会挡掉一些它不认的客户端
//! （比如很旧的浏览器、App 内置浏览器），所以这里只放常见的手机浏览器。
//!
//! 还有一点：每个浏览器说明都固定配一个屏幕尺寸。同一个说明每次都给同样的尺寸，
//! 看起来才像同一台真手机；要是这次说自己是 iPhone、屏幕报安卓尺寸，就很假。
//!
//! Every request carries a note saying "this is which browser". The far end
//! turns away clients it does not recognise (very old browsers, in-app browsers
//! and so on), so we only list common phone browsers here.
//!
//! One more thing: each browser note is paired with a fixed screen size. Giving
//! the same note the same size every time is what makes it look like one real
//! phone; claiming to be an iPhone while reporting an Android screen size looks
//! obviously wrong.

use std::sync::atomic::{AtomicUsize, Ordering};

/// 一条浏览器说明，配一个屏幕尺寸。
/// One browser note, paired with a screen size.
#[derive(Clone, Copy)]
pub struct Entry {
    /// 浏览器说明，也就是 User-Agent。
    /// The browser note, also known as the User-Agent.
    pub agent: &'static str,
    /// 屏幕尺寸，写成 "宽x高"。
    /// Screen size, written as "width x height".
    pub screen: &'static str,
}

/// 万一列表空了就用这条，正常不会走到。
/// Used only if the list somehow ends up empty; should never happen.
const FALLBACK: Entry = Entry {
    agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_3_2 like Mac OS X) \
AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3.1 Mobile/15E148 Safari/604.1",
    screen: "390x844",
};

/// 苹果手机 Safari，覆盖 iOS 15 到 18。
/// iPhone Safari, covering iOS 15 through 18.
const IPHONE: [Entry; 16] = [
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_3_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3.1 Mobile/15E148 Safari/604.1", screen: "390x844" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.2 Mobile/15E148 Safari/604.1", screen: "393x852" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1", screen: "375x812" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1", screen: "430x932" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_6_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Mobile/15E148 Safari/604.1", screen: "414x896" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1", screen: "430x932" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Mobile/15E148 Safari/604.1", screen: "428x926" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_3_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.3 Mobile/15E148 Safari/604.1", screen: "393x852" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Mobile/15E148 Safari/604.1", screen: "428x926" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_1_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.1 Mobile/15E148 Safari/604.1", screen: "360x780" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 16_7_8 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.6 Mobile/15E148 Safari/604.1", screen: "360x780" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 16_6_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.6 Mobile/15E148 Safari/604.1", screen: "390x844" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 16_3_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.3 Mobile/15E148 Safari/604.1", screen: "390x844" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 16_0_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.0 Mobile/15E148 Safari/604.1", screen: "414x896" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 15_8_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/15.6.6 Mobile/15E148 Safari/604.1", screen: "375x812" },
    Entry { agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 15_7_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/15.6.4 Mobile/15E148 Safari/604.1", screen: "414x896" },
];

/// 安卓手机 Chrome。
/// Android Chrome.
const ANDROID: [Entry; 16] = [
    Entry { agent: "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36", screen: "412x915" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 14; Pixel 8 Pro) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Mobile Safari/537.36", screen: "412x892" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 14; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Mobile Safari/537.36", screen: "412x915" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 14; Pixel 7a) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Mobile Safari/537.36", screen: "393x873" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 14; SM-S928B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Mobile Safari/537.36", screen: "384x854" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 14; SM-S928U) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Mobile Safari/537.36", screen: "384x854" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 14; moto g stylus 5G) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36", screen: "432x960" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 14; OPPO Find X5) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36", screen: "412x892" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 13; SM-A536B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/127.0.0.0 Mobile Safari/537.36", screen: "360x800" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 13; SM-A546B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/127.0.0.0 Mobile Safari/537.36", screen: "360x800" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 13; SM-G991B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36", screen: "360x800" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 13; SM-G998B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Mobile Safari/537.36", screen: "360x780" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 13; OnePlus 11) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36", screen: "412x915" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 13; Xiaomi 13) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36", screen: "393x873" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 12; Redmi Note 11) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36", screen: "393x873" },
    Entry { agent: "Mozilla/5.0 (Linux; Android 12; SM-A525F) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Mobile Safari/537.36", screen: "432x960" },
];

/// 轮到第几条了。每次取用往后挪一格，转到末尾就回到开头。
/// Which one is next. Each call moves along one, wrapping back to the start.
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// 一共多少条。
/// How many there are in total.
const TOTAL: usize = IPHONE.len() + ANDROID.len();

/// 按序号取一条。前半是苹果，后半是安卓。
/// Get one by number. The first half is iPhone, the second half is Android.
fn at(index: usize) -> Entry {
    if TOTAL == 0 {
        return FALLBACK;
    }
    let i = index % TOTAL;
    if i < IPHONE.len() {
        IPHONE[i]
    } else {
        ANDROID[i - IPHONE.len()]
    }
}

/// 取下一条浏览器说明和它配的屏幕尺寸。
///
/// 这两个值必须一起用：请求头里写的浏览器，和加密内容里写的浏览器，得是同一个。
/// 头里说自己是 iPhone、加密内容里写另一款，一眼就看出是假的。
///
/// Get the next browser note along with its paired screen size.
///
/// These two must be used together: the browser in the request header and the
/// browser inside the encrypted content have to match. Saying iPhone in the
/// header and something else inside is an obvious giveaway.
pub fn next() -> Entry {
    at(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// 查某条浏览器说明配的屏幕尺寸。
///
/// 列表里有就直接返回；没有的话按名字里的关键词猜一个，并且保证同样的名字每次
/// 都猜到同一个尺寸（用名字算个固定的数来选）。
///
/// Look up the screen size paired with a browser note.
///
/// If it is in our list, return that. Otherwise guess from keywords in the name,
/// making sure the same name always guesses the same size (by turning the name
/// into a fixed number and picking with it).
pub fn screen_for(agent: &str) -> String {
    for i in 0..TOTAL {
        let e = at(i);
        if e.agent == agent {
            return e.screen.to_string();
        }
    }

    let lower = agent.to_lowercase();
    let candidates: &[&str] = if lower.contains("ipad") {
        &["820x1180", "834x1194", "768x1024", "744x1133", "1024x1366"]
    } else if lower.contains("iphone") || lower.contains("ipod") {
        &["390x844", "393x852", "375x812", "414x896", "430x932", "428x926", "360x780"]
    } else {
        &["360x800", "412x915", "393x873", "384x854", "360x780", "412x892", "432x960"]
    };

    candidates[(stable_number(agent) % candidates.len() as u64) as usize].to_string()
}

/// 把一段文字变成一个固定的数字。
///
/// 同样的文字每次都得到同样的数，这样同一个浏览器说明就总配到同一个屏幕尺寸。
/// 不能用系统自带的哈希，那个每次程序启动都不一样。
///
/// Turn a piece of text into a fixed number.
///
/// The same text must always give the same number, so one browser note always
/// gets one screen size. The built-in hash will not do: it changes every time the
/// program starts.
fn stable_number(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 列表不为空且都成对_list_is_filled_and_paired() {
        // 条数太少轮换就没意义了，这里定个下限。
        // Too few entries makes rotation pointless, so this sets a floor.
        let total = TOTAL;
        assert!(total >= 30, "条数太少，轮换意义不大 / too few entries for rotation to matter");

        for i in 0..TOTAL {
            let e = at(i);
            assert!(!e.agent.is_empty(), "浏览器说明不能空 / browser note must not be empty");
            assert!(!e.screen.is_empty(), "屏幕尺寸不能空 / screen size must not be empty");
            assert!(e.screen.contains('x'), "尺寸格式应为 宽x高 / size should look like WxH");
        }
    }

    #[test]
    fn 同一说明总配同一尺寸_same_note_gives_same_size() {
        for i in 0..TOTAL {
            let e = at(i);
            assert_eq!(
                screen_for(e.agent),
                e.screen,
                "列表里的说明应该查到它自己的尺寸 / a listed note should look up its own size"
            );
        }
    }

    #[test]
    fn 没见过的说明也稳定_unknown_note_is_still_stable() {
        let odd = "Mozilla/5.0 (iPhone; something we never listed)";
        let first = screen_for(odd);
        let second = screen_for(odd);
        assert_eq!(first, second, "同样的输入应该给同样的结果 / same input should give same result");
        assert!(!first.is_empty());
    }

    #[test]
    fn 会轮换不会卡在一条_rotation_actually_moves() {
        let a = next().agent;
        let mut moved = false;
        for _ in 0..TOTAL {
            if next().agent != a {
                moved = true;
                break;
            }
        }
        assert!(moved, "取了一圈还是同一条，说明没轮换 / a full lap gave the same note, rotation is stuck");
    }
}
