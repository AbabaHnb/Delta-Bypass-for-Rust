//! 所有可以调的数字都放在这里
//! Every number you might want to change lives here
//!
//! 把这些散在代码各处很容易改漏，所以集中一处。
//! Scattering these through the code makes it easy to miss one, so they are all
//! in a single place.

use std::time::Duration;

// ---------------------------------------------------------------------------
// 网址 / Web addresses
// ---------------------------------------------------------------------------

/// 验证码服务器。
/// The server that hands out picture puzzles.
pub const CAPTCHA_HOST: &str = "https://captcha.platorelay.com";

/// 验证码的接口前缀。
/// The address prefix for picture puzzle requests.
pub const CAPTCHA_API: &str = "https://captcha.platorelay.com/api";

/// 登录服务器的接口前缀。
/// The address prefix for the login server.
pub const AUTH_API: &str = "https://auth.platorelay.com/api";

/// 用来生成测试链接的接口。
/// The address used to make test links.
pub const LINK_API: &str = "https://api.platoboost.net";

/// 生成测试链接时用的浏览器名字。
/// The browser name we send when making test links.
pub const LINK_AGENT: &str = "Platoboost Python Client/1.0";

// ---------------------------------------------------------------------------
// 步骤之间的等待 / Waiting between steps
// ---------------------------------------------------------------------------

/// 两次提交之间必须隔这么久。
///
/// 这是对面服务器的规定，不是我们随便定的。间隔不够它会回
/// "finishing checkpoints too fast" 并让你白等更久，所以别往下调。
///
/// Two submissions must be this far apart.
///
/// This is the far end's rule, not ours. Too close together and it replies
/// "finishing checkpoints too fast", which costs more time than it saves, so do
/// not lower it.
pub const MIN_STEP_GAP: Duration = Duration::from_millis(5000);

/// 在上面那个间隔之外，一开始多留的一点余量。
///
/// 我们只知道自己什么时候把请求发出去，对面看的是它什么时候收到。网络快慢
/// 有波动，所以刚好 5 秒发出去，可能 4.98 秒就到了。这点余量把波动吃掉，
/// 而且会自己变：一直顺就慢慢减少，被拒一次就立刻加回去。
///
/// A little extra on top of the gap above, to start with.
///
/// We only know when we sent a request; the far end goes by when it arrived.
/// Network speed wobbles, so sending at exactly 5 seconds can arrive at 4.98.
/// This extra bit absorbs the wobble, and it tunes itself: it shrinks while
/// things go well and jumps back up the moment we get refused.
pub const GAP_MARGIN_START: Duration = Duration::from_millis(250);

/// 余量最小值，再顺也不会低于这个。
/// The smallest the extra bit ever gets, no matter how well things go.
pub const GAP_MARGIN_MIN: Duration = Duration::from_millis(60);

/// 余量最大值，被拒很多次也不会超过这个。
/// The largest the extra bit ever gets, even after many refusals.
pub const GAP_MARGIN_MAX: Duration = Duration::from_millis(1500);

/// 顺利一次就减少这么多。
/// How much the extra bit shrinks after a good run.
pub const GAP_MARGIN_STEP_DOWN: Duration = Duration::from_millis(40);

/// 被拒一次就增加这么多。
/// How much the extra bit grows after a refusal.
pub const GAP_MARGIN_STEP_UP: Duration = Duration::from_millis(300);

/// 连续顺利几次才减少余量。太急着减会来回抖。
/// How many good runs in a row before shrinking. Shrinking too eagerly makes it
/// bounce up and down.
pub const CLEAN_STEPS_TO_RELAX: u64 = 3;

// ---------------------------------------------------------------------------
// 查钥匙 / Checking for the key
// ---------------------------------------------------------------------------

/// 最多查几次钥匙。
/// How many times at most we check whether the key is ready.
pub const POLL_MAX_ATTEMPTS: usize = 10;

/// 两次查询之间隔多久。
/// How long between checks.
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 提交发出后等多久开始查钥匙。
///
/// 不等提交回话就先去查，两件事一起做，省掉一趟来回的时间。稍微等一下是为了
/// 不做明显太早的无用查询。
///
/// How long after sending a submission we start checking for the key.
///
/// We start checking without waiting for the submission to reply, so both happen
/// at once and we save one round of waiting. The small delay avoids an obviously
/// pointless early check.
pub const POLL_OVERLAP_DELAY: Duration = Duration::from_millis(50);

// ---------------------------------------------------------------------------
// 出错重试 / Retrying after problems
// ---------------------------------------------------------------------------

/// 验证码识别失败时，当场重试几次。
/// How many times we retry on the spot when a picture puzzle is not recognised.
pub const CAPTCHA_MAX_RETRIES: usize = 1;

/// 被对面拦下时重试几次。
/// How many times we retry when the far end holds us back.
pub const STEP_THROTTLE_RETRIES: usize = 2;

/// 被拦下后先歇多久再试。
/// How long we rest after being held back, before trying again.
pub const STEP_THROTTLE_SLEEP: Duration = Duration::from_millis(2000);

/// 提交遇到网络抖动时当场重试几次。
/// How many times we retry a submission when the network hiccups.
pub const STEP_HTTP_RETRIES: usize = 3;

/// 每次网络重试之前歇多久。
/// How long we rest before each network retry.
pub const STEP_RETRY_SLEEP: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------------------
// 走几轮 / How many rounds
// ---------------------------------------------------------------------------

/// 默认走几轮。实际轮数由服务器给的关卡数决定，这只是起点。
/// Default number of rounds. The real number comes from how many checkpoints the
/// server reports; this is only a starting point.
pub const DEFAULT_MAX_ROUNDS: usize = 3;

/// 硬上限，防止万一转不出来一直转。
/// A hard ceiling, so a stuck link cannot loop forever.
pub const MAX_ROUNDS_HARD_CAP: usize = 12;

// ---------------------------------------------------------------------------
// 预备验证码 / Captcha puzzles kept ready in advance
// ---------------------------------------------------------------------------

/// 预备好的题放多久就丢弃。
///
/// 服务器说一道题能用 60 秒。我们只留 30 秒，留足余地。
///
/// How long a ready-made puzzle is kept before being thrown away.
///
/// The server says a puzzle lasts 60 seconds. We only keep ours 30, to leave
/// plenty of room.
pub const POOL_MAX_AGE: Duration = Duration::from_secs(30);

/// 剩余时间不到这么多就不发给别人用了，怕提交的时候正好过期。
/// If a puzzle has less than this much life left we stop handing it out, in case
/// it expires mid-submission.
pub const POOL_USE_MARGIN: Duration = Duration::from_millis(1500);

/// 要题的时候最多等多久；等不到就当场自己做一道。
/// How long a caller waits for a ready-made puzzle; if none turns up it just does
/// one itself.
pub const POOL_TAKE_TIMEOUT: Duration = Duration::from_millis(50);

/// 备题的最快速度：每这么久最多做一道。
///
/// 实测每秒一道可以一直跑（连续 70 次全部正常）；一拥而上就会被服务器限流，
/// 而且限流之后连当场做题也一起失败。这个数字不要往小调。
///
/// Fastest we prepare puzzles: at most one per this long.
///
/// Measured: one per second runs indefinitely (70 in a row, all fine). Rushing
/// several at once gets us rate-limited, and once that happens even puzzles done
/// on the spot start failing. Do not lower this.
pub const POOL_MIN_SLOT_INTERVAL: Duration = Duration::from_millis(950);

/// 同时最多做几道题。
/// How many puzzles we prepare at the same time.
pub const POOL_MAX_INFLIGHT: usize = 2;

/// 后台做题的线程数。它们要抢时间片，所以这个数不等于速度。
/// How many background threads prepare puzzles. They queue for time slots, so
/// this number is not the speed.
pub const POOL_WORKERS: usize = 3;

/// 被服务器拒绝后，第一次歇多久。
/// After the server refuses us, how long we rest the first time.
pub const POOL_BACKOFF_START: Duration = Duration::from_secs(5);

/// 反复被拒时，歇的时间最多到这么长。
/// If refusals keep coming, the rest never grows past this.
pub const POOL_BACKOFF_MAX: Duration = Duration::from_secs(60);

/// 预备池默认放多少道。
/// How many ready-made puzzles we aim to keep by default.
pub const POOL_DEFAULT_SIZE: usize = 30;

// ---------------------------------------------------------------------------
// 网络连接 / Network connections
// ---------------------------------------------------------------------------

/// 建立连接最多等多久。
/// How long we wait at most to open a connection.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// 一个请求整体最多等多久。
/// How long we wait at most for one whole request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

/// 下载图片最多等多久。图片有 45KB 左右，给宽松些。
/// How long we wait at most to download a picture. They are around 45KB, so this
/// is generous.
pub const IMAGE_TIMEOUT: Duration = Duration::from_secs(10);

/// 连接闲着多久还留着不关。
///
/// 留着很重要：45KB 的图片走已经开好的连接大约 25 毫秒，重新开一条要 660 毫秒。
///
/// How long an idle connection is kept open.
///
/// Keeping them matters: a 45KB picture takes around 25 milliseconds over an
/// already-open connection, versus 660 milliseconds over a fresh one.
pub const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

// ---------------------------------------------------------------------------
// 网页接口 / Web interface
// ---------------------------------------------------------------------------

/// 默认端口。
/// Default port.
pub const DEFAULT_PORT: u16 = 2233;

/// 默认监听地址。只想本机访问就用 127.0.0.1。
/// Default listen address. Use 127.0.0.1 if you only want local access.
pub const DEFAULT_HOST: &str = "0.0.0.0";

/// 钥匙记多久。跟钥匙本身的有效期一样长。
/// How long we remember a key. Same as how long the key itself lasts.
pub const CACHE_TTL: Duration = Duration::from_secs(24 * 3600);

/// 记钥匙的文件名。
/// The file we write remembered keys into.
pub const CACHE_FILE: &str = ".key_cache.json";

// ---------------------------------------------------------------------------
// 显示用的固定文字 / Fixed text shown to callers
// ---------------------------------------------------------------------------

/// 接口返回里的制作者。
/// The maker name in the reply.
pub const MADE_BY: &str = "Hasl_Team";

/// 接口返回里的群号。
/// The group number in the reply.
pub const QQ_GROUP: &str = "277707901";
