//! Windows 下的路径修正：让相对路径按程序所在目录算，而不是按当前工作目录。
//!
//! Path fix for Windows: make relative paths follow the program's own folder rather
//! than whatever directory it was started from.
//!
//! ## 为什么要这个
//!
//! 记钥匙的文件 `.key_cache.json` 用的是相对路径。Linux 上一般在服务里指定了工作目
//! 录（systemd 的 `WorkingDirectory=`），没问题。
//!
//! Windows 上不一样：双击 exe 的时候工作目录是 exe 所在目录（还行），但从别的地方
//! 用命令行启动、或者用任务计划程序启动，工作目录可能是 `C:\Windows\System32` 那种
//! 地方 —— 结果钥匙文件写到那儿去了，或者压根写不进去（没权限）。
//!
//! 所以启动时先把工作目录切到 exe 所在目录，这样两个平台上行为一致：文件总是落在
//! 程序旁边。
//!
//! ## Why this exists
//!
//! The remembered-keys file `.key_cache.json` uses a relative path. On Linux the
//! working directory is normally set by the service (`WorkingDirectory=` in systemd),
//! so that is fine.
//!
//! Windows is different: double-clicking the exe gives a working directory of the exe's
//! own folder (fine), but starting it from a command line elsewhere, or through Task
//! Scheduler, can leave the working directory somewhere like `C:\Windows\System32` — so
//! the keys file lands there, or cannot be written at all for lack of permission.
//!
//! So at startup we move the working directory to wherever the exe lives, which makes
//! both platforms behave the same: the file always sits next to the program.

/// 把工作目录切到程序所在目录。
///
/// 只在 Windows 上做。Linux 上服务通常自己指定了工作目录，不该覆盖人家的设置。
///
/// 切不过去也不报错 —— 顶多是钥匙文件位置跟预期不同，不该因此让程序起不来。
///
/// Move the working directory to wherever the program lives.
///
/// Windows only. On Linux the service usually sets its own working directory, and we
/// should not override that.
///
/// A failure here is not reported — at worst the keys file ends up somewhere else, which
/// is no reason to stop the program from starting.
pub fn use_program_folder() {
    #[cfg(windows)]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(folder) = exe.parent() {
                let _ = std::env::set_current_dir(folder);
            }
        }
    }
}
