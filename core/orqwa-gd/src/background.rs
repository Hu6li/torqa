//! Lowers the priority of the threads that build a world while it is ridden (ADR 0040, 2d), so
//! drawing the scene comes first. Through the system's C library, which every build links
//! already.

use std::ffi::c_int;

#[cfg(any(target_os = "linux", target_os = "android"))]
unsafe extern "C" {
    fn setpriority(which: c_int, who: std::ffi::c_uint, priority: c_int) -> c_int;
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThread() -> *mut std::ffi::c_void;
    fn SetThreadPriority(thread: *mut std::ffi::c_void, priority: c_int) -> c_int;
}

#[cfg(target_vendor = "apple")]
unsafe extern "C" {
    fn pthread_set_qos_class_self_np(class: std::ffi::c_uint, relative: c_int) -> c_int;
}

/// Lowers the calling thread's priority below the drawing's. Left as it is where that fails.
pub fn lower_this_thread() {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        const PRIO_PROCESS: c_int = 0;
        // Android's THREAD_PRIORITY_BACKGROUND.
        const BACKGROUND: c_int = 10;
        // SAFETY: scalars only. Linux keeps a nice value per thread, and `who` 0 names the
        // calling one.
        let _ = unsafe { setpriority(PRIO_PROCESS, 0, BACKGROUND) };
    }
    #[cfg(windows)]
    {
        const THREAD_PRIORITY_BELOW_NORMAL: c_int = -1;
        // SAFETY: the pseudo-handle of the calling thread, which needs no closing.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
    }
    #[cfg(target_vendor = "apple")]
    {
        // Utility also lowers the threads' disk priority; they write the chunks to the store.
        const QOS_CLASS_UTILITY: std::ffi::c_uint = 0x11;
        // SAFETY: scalars only; it changes the calling thread alone.
        let _ = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0) };
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    /// The calling thread's nice value, from Linux's per-thread status.
    fn nice() -> i64 {
        let stat = std::fs::read_to_string("/proc/thread-self/stat").unwrap();
        // The name in parentheses may hold spaces; nice is the 17th field after it.
        let after_name = &stat[stat.rfind(')').unwrap() + 2..];
        after_name.split(' ').nth(16).unwrap().parse().unwrap()
    }

    #[test]
    fn a_lowered_thread_gives_way_and_the_others_keep_their_priority() {
        let before = nice();
        if before >= 10 {
            // Already as low as it lowers to, e.g. tests run under `nice`.
            return;
        }
        let lowered = std::thread::spawn(|| {
            super::lower_this_thread();
            nice()
        })
        .join()
        .unwrap();
        assert!(lowered > before, "{lowered} not below {before}");
        assert_eq!(nice(), before, "only the calling thread");
    }
}
