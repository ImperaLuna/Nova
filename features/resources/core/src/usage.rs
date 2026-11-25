use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub total: u64,
    pub idle: u64,
}

impl CpuTimes {
    pub fn parse(stat: &str) -> Option<Self> {
        let line = stat.lines().find(|line| line.starts_with("cpu "))?;
        let columns: Vec<u64> =
            line.split_whitespace().skip(1).take(8).map(|n| n.parse().ok()).collect::<Option<_>>()?;
        let [user, nice, system, idle, iowait, irq, softirq, steal] = columns[..] else { return None };
        Some(Self { total: user + nice + system + idle + iowait + irq + softirq + steal, idle: idle + iowait })
    }

    pub fn read(proc_root: &Path) -> Option<Self> {
        Self::parse(&std::fs::read_to_string(proc_root.join("stat")).ok()?)
    }

    pub fn busy_since(&self, earlier: &Self) -> f32 {
        let total = self.total.saturating_sub(earlier.total);
        let idle = self.idle.saturating_sub(earlier.idle);
        match total {
            0 => 0.0,
            total => percent(total.saturating_sub(idle), total),
        }
    }
}

pub fn percent(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        return 0.0;
    }
    (part as f64 / whole as f64 * 100.0).clamp(0.0, 100.0) as f32
}

pub fn memory(meminfo: &str) -> Option<(u64, u64)> {
    let field = |key: &str| {
        meminfo.lines().find_map(|line| {
            let kib: u64 = line.strip_prefix(key)?.strip_prefix(':')?.trim().strip_suffix("kB")?.trim().parse().ok()?;
            Some(kib * 1024)
        })
    };
    let total = field("MemTotal")?;
    let available = field("MemAvailable")?;
    Some((total.saturating_sub(available), total))
}

pub fn read_memory(proc_root: &Path) -> Option<(u64, u64)> {
    memory(&std::fs::read_to_string(proc_root.join("meminfo")).ok()?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub program: Option<PathBuf>,
    pub uid: Option<u32>,
    pub ticks: u64,
    pub resident_pages: u64,
}

impl Process {
    pub fn parse(pid: u32, stat: &str) -> Option<Self> {
        let open = stat.find('(')?;
        let close = stat.rfind(')')?;
        let name = stat.get(open + 1..close)?.to_string();
        let fields: Vec<&str> = stat.get(close + 1..)?.split_whitespace().collect();
        let field = |number: usize| fields.get(number - 3)?.parse::<u64>().ok();
        Some(Self {
            pid,
            ppid: u32::try_from(field(4)?).ok()?,
            name,
            program: None,
            uid: None,
            ticks: field(14)? + field(15)?,
            resident_pages: field(24)?,
        })
    }
}

pub fn processes(proc_root: &Path) -> Vec<Process> {
    let Ok(entries) = std::fs::read_dir(proc_root) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let dir = entry.path();
            let mut process = Process::parse(pid, &std::fs::read_to_string(dir.join("stat")).ok()?)?;
            process.program = std::fs::read_link(dir.join("exe")).ok();
            process.uid = std::fs::metadata(&dir).ok().map(|meta| meta.uid());
            Some(process)
        })
        .collect()
}

#[derive(Debug, Default)]
pub struct ProcessTicks(HashMap<u32, u64>);

impl ProcessTicks {
    pub fn since_last(&mut self, now: &[Process]) -> Vec<u64> {
        let used =
            now.iter().map(|process| process.ticks.saturating_sub(*self.0.get(&process.pid).unwrap_or(&0))).collect();
        self.0 = now.iter().map(|process| (process.pid, process.ticks)).collect();
        used
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_busy_is_the_share_of_non_idle_time() {
        let before = CpuTimes::parse("cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 1 2 3 4\n").unwrap();
        let after = CpuTimes::parse("cpu  150 0 150 800 100 0 0 0 50 0\n").unwrap();
        assert_eq!(before, CpuTimes { total: 1000, idle: 800 });
        assert_eq!(after.busy_since(&before), 50.0, "guest time isn't counted twice");
        assert_eq!(after.busy_since(&after), 0.0);
        assert_eq!(CpuTimes::parse("cpu  1 2\n"), None);
    }

    #[test]
    fn memory_in_use_is_what_isnt_available() {
        let meminfo = "MemTotal:       32768000 kB\nMemFree:         1000000 kB\nMemAvailable:   22768000 kB\nBuffers:          100 kB\n";
        assert_eq!(memory(meminfo), Some((10_000_000 * 1024, 32_768_000 * 1024)));
        assert_eq!(memory("MemTotal: 1 kB\n"), None);
    }

    #[test]
    fn process_names_may_hold_parentheses_and_spaces() {
        let stat = "4242 (Web Content (x)) S 1 4242 4242 0 -1 4194560 2954 0 0 0 610 40 0 0 20 0 30 0 5221 2510000000 61440 18446744073709551615 1 1 0 0 0 0 0 4096 0 0 0 0 17 3 0 0 0 0 0";
        let process = Process::parse(4242, stat).unwrap();
        assert_eq!(process.name, "Web Content (x)");
        assert_eq!(process.ppid, 1);
        assert_eq!(process.ticks, 650);
        assert_eq!(process.resident_pages, 61440);
        assert_eq!(Process::parse(1, "1 (init) S 0"), None);
    }

    #[test]
    fn ticks_count_from_the_last_look() {
        let process =
            |pid, ticks| Process { pid, ppid: 1, name: "x".into(), program: None, uid: None, ticks, resident_pages: 1 };
        let mut ticks = ProcessTicks::default();
        assert_eq!(ticks.since_last(&[process(1, 100)]), [100]);
        assert_eq!(ticks.since_last(&[process(1, 130), process(2, 5)]), [30, 5]);
        assert_eq!(ticks.since_last(&[process(2, 5)]), [0]);
    }

    #[test]
    fn reads_this_machine() {
        let proc_root = Path::new(nova_process::procfs::PROC);
        assert!(CpuTimes::read(proc_root).is_some_and(|times| times.total > 0));
        assert!(read_memory(proc_root).is_some_and(|(used, total)| used <= total && total > 0));
        let me = std::process::id();
        let mine = processes(proc_root).into_iter().find(|process| process.pid == me).unwrap();
        assert!(mine.resident_pages > 0);
        assert_eq!(mine.program, std::env::current_exe().ok());
        // SAFETY: getuid only reads the calling process's uid.
        assert_eq!(mine.uid, Some(unsafe { libc::getuid() }));
    }
}
