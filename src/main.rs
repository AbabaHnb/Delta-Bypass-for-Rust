//! Delta 绕过器 —— 命令行入口
//! Delta bypass — command line entry point
//!
//! 三种用法：
//!
//! ```text
//! delta-bypass "<链接>"              绕过一条
//! delta-bypass --generate 3          生成 3 条测试链接并绕过
//! delta-bypass --serve               开成网页接口
//! ```
//!
//! 还有两个调试用的：`--img` 拿本地图片文件试一下认得对不对，`--pool-stats` 单独看
//! 预备池的状况。
//!
//! Three ways to use it:
//!
//! ```text
//! delta-bypass "<link>"              bypass one
//! delta-bypass --generate 3          make 3 test links and bypass them
//! delta-bypass --serve               run as a web interface
//! ```
//!
//! Two more for debugging: `--img` tries a local picture file to see whether it is read
//! correctly, and `--pool-stats` watches the puzzle store on its own.

use std::time::{Duration, Instant};

use clap::{CommandFactory, Parser};

use delta_bypass::{api, auth, chain, config, link, net, platform, pool, solver, timing};

/// 命令行参数。
/// Command line options.
#[derive(Parser)]
#[command(
    name = "delta-bypass",
    version,
    about = "Delta 钥匙绕过器 / Delta key bypass",
    long_about = "自动处理验证码和关卡，从链接拿到钥匙。\n\
                  Handles picture puzzles and checkpoints automatically, turning a link into a key."
)]
struct Options {
    /// 要绕过的东西：链接、通行串，或者存了这些的文件
    /// What to bypass: a link, a pass string, or a file holding them
    target: Option<String>,

    /// 生成几条测试链接来试
    /// How many test links to make and try
    #[arg(short = 'g', long = "generate", default_value_t = 0)]
    generate: usize,

    /// 只报结果，不报过程
    /// Report the result only, not the progress
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,

    /// 最多走几轮（实际会按服务器说的关卡数往上调）
    /// Round cap, raised to match however many checkpoints the server reports
    #[arg(long = "max-rounds", default_value_t = config::DEFAULT_MAX_ROUNDS)]
    max_rounds: usize,

    /// 只生成链接，不绕过
    /// Only make links, do not bypass
    #[arg(long = "no-auto")]
    no_auto: bool,

    /// 开成网页接口
    /// Run as a web interface
    #[arg(long = "serve")]
    serve: bool,

    /// 网页接口监听哪个端口
    /// Which port the web interface listens on
    #[arg(short = 'p', long = "port", default_value_t = config::DEFAULT_PORT)]
    port: u16,

    /// 网页接口监听哪个地址（只想本机能用就填 127.0.0.1）
    /// Which address it listens on (use 127.0.0.1 for local access only)
    #[arg(long = "host", default_value = config::DEFAULT_HOST)]
    host: String,

    /// 预备池存多少道，0 就是不备
    /// How many puzzles to keep ready; 0 turns it off
    #[arg(long = "prepared", default_value_t = config::POOL_DEFAULT_SIZE)]
    prepared: usize,

    /// 调试：拿本地图片文件试一下
    /// Debug: try a local picture file
    #[arg(long = "img")]
    img: Option<String>,

    /// 调试：那张图是哪种题（driftodd 或 coherence）
    /// Debug: which kind of puzzle that picture is (driftodd or coherence)
    #[arg(long = "img-type", default_value = "driftodd")]
    img_type: String,

    /// 调试：同一张图反复算几次，看耗时
    /// Debug: work out the same picture several times, to see the timing
    #[arg(long = "bench", default_value_t = 1)]
    bench: usize,

    /// 调试：只看预备池状况，不绕过
    /// Debug: only watch the puzzle store, do not bypass
    #[arg(long = "pool-stats")]
    pool_stats: bool,

    /// 调试：看多少秒
    /// Debug: how many seconds to watch for
    #[arg(long = "pool-watch-secs", default_value_t = 60)]
    pool_watch_secs: u64,
}

fn main() {
    // Windows 上把工作目录切到 exe 旁边，免得钥匙文件写到莫名其妙的地方去。
    // Linux 上不动，服务通常自己指定了工作目录。
    //
    // On Windows, move the working directory next to the exe, so the keys file does not
    // land somewhere unexpected. Left alone on Linux, where the service usually sets its
    // own.
    platform::use_program_folder();

    let options = Options::parse();

    if options.pool_stats {
        watch_pool(options.prepared, options.pool_watch_secs);
        return;
    }

    if let Some(path) = &options.img {
        try_picture(path, &options.img_type, options.bench);
        return;
    }

    if options.serve {
        api::serve(&options.host, options.port, options.prepared);
        return;
    }

    bypass_links(&options);
}

// ---------------------------------------------------------------------------
// 调试：试一张本地图片 / Debug: try a local picture
// ---------------------------------------------------------------------------

/// 拿本地图片文件试一下，看认得对不对、多快。
///
/// 排查"是图片认错了还是网络出问题"的时候很有用 —— 这条路完全不联网。
///
/// Try a local picture file, to see whether it is read correctly and how fast.
///
/// Handy when working out whether a picture was misread or the network is at fault —
/// this route does not touch the network at all.
fn try_picture(path: &str, kind: &str, times: usize) {
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("无法读取文件 / Cannot read file: {}", path);
        std::process::exit(1);
    };

    let started = Instant::now();
    let (choice, method) = solver::solve(&bytes, kind);
    let first_ms = started.elapsed().as_secs_f64() * 1000.0;

    // 只算一次就直接报。
    // Only asked for one, so just report it.
    if times <= 1 {
        match choice {
            Some((x, y)) => println!("({:.2}, {:.2}) [{}] {:.1}ms", x, y, method, first_ms),
            None => println!("识别未命中 / No match [{}] {:.1}ms", method, first_ms),
        }
        return;
    }

    // 多算几次看快慢。第一次通常偏慢（要预热），所以最快那次更有参考价值。
    // Several times, to see the speed. The first is usually slower (warming up), so the
    // fastest one is the more useful figure.
    let mut all = Vec::with_capacity(times);
    for _ in 0..times {
        let one = Instant::now();
        let _ = solver::solve(&bytes, kind);
        all.push(one.elapsed().as_secs_f64() * 1000.0);
    }

    let fastest = all.iter().cloned().fold(f64::INFINITY, f64::min);
    let average = all.iter().sum::<f64>() / all.len() as f64;

    match choice {
        Some((x, y)) => println!(
            "({:.2}, {:.2}) [{}] 首次={:.1}ms 最快={:.1}ms 平均={:.1}ms 样本数={}",
            x, y, method, first_ms, fastest, average, times
        ),
        None => println!(
            "识别未命中 / No match [{}] 首次={:.1}ms 最快={:.1}ms 平均={:.1}ms 样本数={}",
            method, first_ms, fastest, average, times
        ),
    }
}

// ---------------------------------------------------------------------------
// 调试：看预备池 / Debug: watch the puzzle store
// ---------------------------------------------------------------------------

/// 单独跑预备池，看它填得怎么样、稳不稳。
///
/// 每秒打一行。想确认"存这么多会不会被限流"就看 `被拒` 那一栏，一直是 0 就没问题。
///
/// Run the puzzle store on its own, to see how it fills and how steady it holds.
///
/// One line per second. To check whether a given size gets rate-limited, watch the
/// refused column — all zeros means it is fine.
fn watch_pool(want: usize, seconds: u64) {
    println!("[预备池] 目标容量={} 观测时长={}s / Target capacity {}, observing {}s", want, seconds, want, seconds);

    net::warm_up_captcha();
    pool::start(want, (*net::captcha_client()).clone());

    let started = Instant::now();
    let mut last_second = 0u64;

    loop {
        let elapsed = started.elapsed().as_secs();

        if elapsed != last_second {
            last_second = elapsed;
            let s = pool::snapshot();

            println!(
                "  t={:>3}s 可用={:>2}/{} 最早={:>4.1}s 最新={:>4.1}s 已生成={} 已发放={} 已过期={} 被拒={} 退避={}ms",
                elapsed,
                s.usable,
                s.want,
                s.oldest_secs,
                s.newest_secs,
                s.made,
                s.handed_out,
                s.expired,
                s.refused,
                s.penalty_ms
            );
        }

        if started.elapsed().as_secs() >= seconds {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ---------------------------------------------------------------------------
// 正事：绕过链接 / The real job: bypassing links
// ---------------------------------------------------------------------------

/// 绕过一条或几条链接。
/// Bypass one or several links.
fn bypass_links(options: &Options) {
    let talk = !options.quiet;
    let overall_start = Instant::now();

    // 一边开连接、一边备题，一边去要链接 —— 都不用等对方。
    // Open connections and prepare puzzles while asking for links — none waits on the
    // others.
    let warming = std::thread::spawn(net::warm_up_all);
    if options.prepared > 0 {
        pool::start(options.prepared, (*net::captcha_client()).clone());
    }

    // 先把要绕过的通行串凑齐。
    // Gather the pass strings to work through.
    let tickets: Vec<String> = if options.generate > 0 {
        if talk {
            println!("[准备] 正在生成 {} 条测试链接 / Generating {} test links", options.generate, options.generate);
        }

        let urls = link::create_many(
            options.generate,
            link::SERVICE_ANDROID,
            Duration::from_millis(300),
        );

        if talk {
            println!("[准备] 已获取 {} 条 / Obtained {}", urls.len(), urls.len());
        }

        urls.iter().map(|u| auth::extract_ticket(u)).collect()
    } else if let Some(given) = &options.target {
        // 命令行参数才允许从文件读凭据（一行一条那种）。
        // Only a command line argument may read credentials from a file (one per line).
        vec![auth::extract_ticket_from_arg(given)]
    } else {
        let _ = Options::command().print_help();
        std::process::exit(1);
    };

    // 只要链接不绕过。
    // Only make links, without bypassing.
    if options.no_auto {
        let _ = warming.join();
        for t in tickets {
            println!("https://auth.platorelay.com/a?d={}", t);
        }
        return;
    }

    let _ = warming.join();

    // 等一下预备池，让第一条也能用上现成的题。
    // Give the store a moment, so even the first link gets a ready-made puzzle.
    if options.prepared > 0 {
        pool::wait_until_ready(Duration::from_secs(3));
    }

    // ---- 一条一条绕 ----
    // ---- Work through them one at a time ----
    let mut results: Vec<(Option<String>, timing::Timer, f64)> = Vec::new();

    for (index, ticket) in tickets.iter().enumerate() {
        let one_start = Instant::now();
        let outcome = chain::run(ticket, talk, options.max_rounds);
        let wall_secs = one_start.elapsed().as_secs_f64();

        if let Some(key) = &outcome.key {
            println!("\n{}", "=".repeat(60));
            println!("[成功] 第{}条密钥 / Key #{}: {}", index + 1, index + 1, key);
            println!("[成功] 耗时 {:.1}s / Elapsed {:.1}s", wall_secs, wall_secs);
            println!("[成功] 阶段明细 / Stage breakdown:");
            println!("{}", outcome.timer.summary());
            println!("{}", "=".repeat(60));
        } else if talk {
            println!("\n[失败] 第{}条未获取密钥 / No key for #{}", index + 1, index + 1);
            println!("[失败] 终止于: {}", outcome.stopped_at);
            if !outcome.timer.is_empty() {
                println!("[失败] 耗时 {:.1}s / Elapsed {:.1}s", wall_secs, wall_secs);
                println!("[失败] 阶段明细 / Stage breakdown:");
                println!("{}", outcome.timer.summary());
            }
        }

        results.push((outcome.key, outcome.timer, wall_secs));
    }

    // ---- 总结 ----
    // ---- Wrap up ----
    let total_secs = overall_start.elapsed().as_secs_f64();
    let succeeded = results.iter().filter(|r| r.0.is_some()).count();

    // 把所有条的各步骤耗时合起来，看整体时间花在哪。
    // Add up every link's step timings, to see where the time went overall.
    let mut combined = timing::Timer::new();
    for (_, one, _) in &results {
        for (step, secs) in one.iter() {
            combined.add(step, *secs);
        }
    }

    println!("\n{}", "=".repeat(60));
    println!("[汇总] 成功 {}/{} 条 / {} of {} succeeded", succeeded, tickets.len(), succeeded, tickets.len());
    println!("[汇总] 总耗时 {:.1}s / {:.1}s total", total_secs, total_secs);

    if !tickets.is_empty() {
        println!(
            "[汇总] 平均每条 {:.1}s / {:.1}s per link on average",
            total_secs / tickets.len() as f64,
            total_secs / tickets.len() as f64
        );
    }

    if !combined.is_empty() {
        println!("[汇总] 阶段合计 / Stage totals:");
        println!("{}", combined.summary());
    }

    println!("{}", "=".repeat(60));
}
