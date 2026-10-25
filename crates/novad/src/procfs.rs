//! Facts about processes from `/proc`.

/// Proportional set size of `pid` in KiB, from `/proc/<pid>/smaps_rollup`.
pub fn pss_kib(pid: u32) -> Option<u64> {
    let rollup = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?;
    rollup.lines().find_map(|line| line.strip_prefix("Pss:")?.trim().strip_suffix("kB")?.trim().parse().ok())
}

/// When the system booted, in Unix seconds: `btime` in `/proc/stat`.
pub fn boot_time() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    stat.lines().find_map(|line| line.strip_prefix("btime ")?.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_own_pss() {
        assert!(super::pss_kib(std::process::id()).is_some_and(|kib| kib > 0));
        assert_eq!(super::pss_kib(u32::MAX), None);
    }

    #[test]
    fn boot_time_is_in_the_past() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        assert!(super::boot_time().is_some_and(|booted| booted > 0 && booted <= now));
    }
}
