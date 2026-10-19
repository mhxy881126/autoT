//! 盘中防休眠。Windows 用 SetThreadExecutionState，macOS 用 caffeinate 子进程。
//! 只阻止系统睡眠，不强制点亮屏幕。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, OnceLock};

static ON: AtomicBool = AtomicBool::new(false);
static TX: OnceLock<mpsc::Sender<bool>> = OnceLock::new();

/// 所有 enable/disable 都在同一根 OS 线程里执行。
/// Windows 的 ES_CONTINUOUS 是线程级的；异步任务换线程会让“释放”落空。
pub fn set(on: bool) {
    if ON.swap(on, Ordering::SeqCst) == on {
        return;
    }
    let tx = TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<bool>();
        let _ = std::thread::Builder::new()
            .name("awake-guard".to_string())
            .spawn(move || {
                for want in rx {
                    if want {
                        imp::enable();
                    } else {
                        imp::disable();
                    }
                }
            });
        tx
    });
    let _ = tx.send(on);
}

pub fn is_on() -> bool {
    ON.load(Ordering::SeqCst)
}

#[cfg(windows)]
mod imp {
    #[link(name = "kernel32")]
    extern "system" {
        fn SetThreadExecutionState(flags: u32) -> u32;
    }

    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;

    pub fn enable() {
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        }
    }

    pub fn disable() {
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::process::{Child, Command};
    use std::sync::Mutex;

    static CHILD: Mutex<Option<Child>> = Mutex::new(None);

    pub fn enable() {
        let mut guard = match CHILD.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if guard.is_some() {
            return;
        }
        let pid = std::process::id().to_string();
        if let Ok(child) = Command::new("/usr/bin/caffeinate")
            .args(["-i", "-w", &pid])
            .spawn()
        {
            *guard = Some(child);
        }
    }

    pub fn disable() {
        if let Ok(mut guard) = CHILD.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub fn enable() {}
    pub fn disable() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_without_panic() {
        set(false);
        set(true);
        assert!(is_on());
        set(false);
        assert!(!is_on());
    }
}
