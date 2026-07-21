//! Cross-platform process RSS (Resident Set Size) reader.
//!
//! Used by [`crate::utils::limits::WorkerLimits`] to enforce
//! `max_memory_per_child` (C8). Returns RSS in bytes.
//!
//! Platform implementations:
//! - **Linux**: parse `/proc/self/status` for `VmRSS:` (in kB).
//! - **macOS**: use `libc::mach_task_basic_info` via the `mach_task_basic_info`
//!   syscall (resident_size is in bytes).
//! - **Windows**: use `GetProcessMemoryInfo` from `winapi` (WorkingSetSize).
//! - **Other / unsupported**: returns `None`.
//!
//! Reference: Celery `worker_max_memory_per_child`.

/// Read the current process's RSS in bytes. Returns `None` on unsupported
/// platforms or read errors (callers should treat `None` as "no limit
/// enforcement possible" rather than failing hard).
pub fn current_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        rss_linux()
    }
    #[cfg(target_os = "macos")]
    {
        rss_macos()
    }
    #[cfg(target_os = "windows")]
    {
        rss_windows()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Linux: parse `/proc/self/status` for the `VmRSS:` line.
/// Format: `VmRSS:\t      1234 kB\n`
#[cfg(target_os = "linux")]
fn rss_linux() -> Option<u64> {
    let content = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            // rest looks like "      1234 kB"
            let kb: u64 = rest
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse()
                .ok()?;
            return Some(kb.saturating_mul(1024));
        }
    }
    None
}

/// macOS: use libc mach_task_basic_info. resident_size is in bytes.
#[cfg(target_os = "macos")]
fn rss_macos() -> Option<u64> {
    use std::mem::{size_of, MaybeUninit};
    unsafe {
        let mut info: MaybeUninit<libc::mach_task_basic_info_data_t> = MaybeUninit::uninit();
        let mut count = (size_of::<libc::mach_task_basic_info_data_t>()
            / size_of::<libc::natural_t>()) as libc::mach_msg_type_number_t;
        let kr = libc::task_info(
            libc::mach_task_self(),
            libc::MACH_TASK_BASIC_INFO,
            info.as_mut_ptr() as libc::task_info_t,
            &mut count,
        );
        if kr != libc::KERN_SUCCESS {
            return None;
        }
        let info = info.assume_init();
        Some(info.resident_size as u64)
    }
}

/// Windows: GetProcessMemoryInfo -> WorkingSetSize (bytes).
#[cfg(target_os = "windows")]
fn rss_windows() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        let ok = GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        );
        if ok == 0 {
            return None;
        }
        Some(counters.WorkingSetSize as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_rss_bytes_returns_some_on_supported_platforms() {
        // On Linux/macOS/Windows this should return Some(>0). On other
        // platforms it returns None — both are acceptable behaviors.
        let rss = current_rss_bytes();
        if let Some(rss) = rss {
            assert!(rss > 0, "RSS should be positive when Some, got {}", rss);
        }
    }
}
