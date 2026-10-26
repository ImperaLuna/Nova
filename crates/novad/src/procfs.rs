//! Facts from `/proc`, each parsed in one place: a process's proportional
//! memory and when the machine booted.

use std::path::Path;

/// The real `/proc`; tests read a recorded copy instead.
pub const PROC: &str = "/proc";

/// Proportional set size of `pid` in KiB, from `<proc_root>/<pid>/smaps_rollup`:
/// its share of what it maps with others counted once, unlike resident
/// memory, which counts shared libraries and GPU driver mappings in every
/// process. Only readable for the user's own processes.
pub fn pss_kib(proc_root: &Path, pid: u32) -> Option<u64> {
    parse_pss_kib(&std::fs::read_to_string(proc_root.join(pid.to_string()).join("smaps_rollup")).ok()?)
}

/// `Pss:` from an `smaps_rollup` file's text, in KiB.
fn parse_pss_kib(smaps_rollup: &str) -> Option<u64> {
    smaps_rollup.lines().find_map(|line| line.strip_prefix("Pss:")?.trim().strip_suffix("kB")?.trim().parse().ok())
}

/// When the system booted, in Unix seconds: `btime` in `/proc/stat`.
pub fn boot_time() -> Option<u64> {
    parse_boot_time(&std::fs::read_to_string(Path::new(PROC).join("stat")).ok()?)
}

/// `btime` from `/proc/stat`'s text.
fn parse_boot_time(stat: &str) -> Option<u64> {
    stat.lines().find_map(|line| line.strip_prefix("btime ")?.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_own_pss() {
        assert!(pss_kib(Path::new(PROC), std::process::id()).is_some_and(|kib| kib > 0));
        assert_eq!(pss_kib(Path::new(PROC), u32::MAX), None);
        assert_eq!(parse_pss_kib("Rss:  2048 kB\nPss:   1536 kB\n"), Some(1536));
        assert_eq!(parse_pss_kib("Rss:  2048 kB\n"), None);
    }

    #[test]
    fn boot_time_from_proc_stat() {
        let stat = "cpu  1 2 3 4 5 6 7 0 0 0\nintr 0\nctxt 9\nbtime 1793000000\nprocesses 1\n";
        assert_eq!(parse_boot_time(stat), Some(1_793_000_000));
        assert_eq!(parse_boot_time("cpu 1 2 3\n"), None);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        assert!(boot_time().is_some_and(|booted| booted > 0 && booted <= now));
    }
}
