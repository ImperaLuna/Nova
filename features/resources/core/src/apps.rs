use super::usage::Process;
use nova_resources_api::AppUsage;
use std::collections::HashMap;
use std::path::Path;

const SESSION_PROGRAMS: [&str; 4] = ["niri", "Hyprland", "sway", "systemd"];

pub const NOVA_KEY: &str = "nova";
const NOVA_NAME: &str = "Nova";

pub fn key(process: &Process, me: Me) -> String {
    if me.owns(process) {
        return NOVA_KEY.to_string();
    }
    match &process.program {
        Some(program) => program.to_string_lossy().into_owned(),
        None => format!("comm:{}", process.name),
    }
}

pub fn name(process: &Process, me: Me) -> String {
    if me.owns(process) {
        return NOVA_NAME.to_string();
    }
    let Some(file) = process.program.as_deref().and_then(Path::file_name) else { return process.name.clone() };
    let file = file.to_string_lossy();
    let file = file.strip_suffix(" (deleted)").unwrap_or(&file);
    let file = file.strip_prefix('.').unwrap_or(file);
    file.strip_suffix("-wrapped").unwrap_or(file).to_string()
}

#[derive(Debug, Clone, Copy)]
pub struct Me {
    pub pid: u32,
    pub uid: u32,
}

impl Me {
    pub fn current() -> Self {
        // SAFETY: getuid only reads the calling process's uid.
        Self { pid: std::process::id(), uid: unsafe { libc::getuid() } }
    }

    fn owns(&self, process: &Process) -> bool {
        process.pid == self.pid || process.ppid == self.pid
    }

    fn protects(&self, members: &[&Process]) -> bool {
        members.iter().any(|process| self.owns(process))
            || members.iter().any(|process| SESSION_PROGRAMS.contains(&name(process, *self).as_str()))
            || !members.iter().any(|process| process.uid == Some(self.uid))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Look<'a> {
    pub processes: &'a [Process],
    pub used: &'a [u64],
    pub total_ticks: u64,
}

pub fn top(look: Look, page: u64, me: Me, top: usize, pss: impl Fn(u32) -> Option<u64>) -> Vec<AppUsage> {
    let Look { processes, used, total_ticks } = look;
    let mut groups: HashMap<String, (Vec<&Process>, u64)> = HashMap::new();
    for (process, ticks) in processes.iter().zip(used) {
        if process.resident_pages == 0 {
            continue;
        }
        let group = groups.entry(key(process, me)).or_default();
        group.0.push(process);
        group.1 += ticks;
    }
    let mut apps: Vec<AppUsage> = groups
        .iter()
        .map(|(key, (members, ticks))| AppUsage {
            name: name(members[0], me),
            processes: u32::try_from(members.len()).unwrap_or(u32::MAX),
            cpu: super::usage::percent(*ticks, total_ticks),
            memory: members.iter().map(|process| process.resident_pages * page).sum(),
            protected: me.protects(members),
            key: key.clone(),
        })
        .collect();
    apps.sort_by(|a, b| b.memory.cmp(&a.memory).then_with(|| a.name.cmp(&b.name)));
    let nova = apps.iter().skip(top).position(|app| app.key == NOVA_KEY).map(|at| apps.remove(top + at));
    apps.truncate(top);
    apps.extend(nova);
    for app in &mut apps {
        app.memory =
            groups[&app.key].0.iter().map(|process| pss(process.pid).unwrap_or(process.resident_pages * page)).sum();
    }
    apps.sort_by(|a, b| b.memory.cmp(&a.memory).then_with(|| a.name.cmp(&b.name)));
    apps
}

pub fn end(processes: &[Process], key: &str, me: Me) -> Result<usize, String> {
    let members: Vec<&Process> = processes.iter().filter(|process| self::key(process, me) == key).collect();
    if members.is_empty() {
        return Err(format!("no app {key} is running"));
    }
    if me.protects(&members) {
        return Err(format!("{} can't be ended from Nova", name(members[0], me)));
    }
    let mut ended = 0;
    for process in members.iter().filter(|process| process.uid == Some(me.uid)) {
        let Ok(pid) = libc::pid_t::try_from(process.pid) else { continue };
        // SAFETY: kill only sends a signal; the pid was just read from /proc.
        if unsafe { libc::kill(pid, libc::SIGTERM) } == 0 {
            ended += 1;
        }
    }
    Ok(ended)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const ME: Me = Me { pid: 100, uid: 1000 };

    fn process(pid: u32, ppid: u32, program: &str, uid: u32, pages: u64) -> Process {
        Process {
            pid,
            ppid,
            name: "comm".into(),
            program: (!program.is_empty()).then(|| PathBuf::from(program)),
            uid: Some(uid),
            ticks: 0,
            resident_pages: pages,
        }
    }

    #[test]
    fn an_apps_processes_count_together_and_most_memory_comes_first() {
        let processes = [
            process(1, 1, "/nix/store/x-firefox/lib/firefox/.firefox-wrapped", 1000, 100),
            process(2, 1, "/usr/bin/foot", 1000, 300),
            process(3, 1, "/nix/store/x-firefox/lib/firefox/.firefox-wrapped", 1000, 250),
            process(4, 1, "", 0, 50),
            process(5, 2, "/usr/bin/kworker", 0, 0),
        ];
        let apps =
            top(Look { processes: &processes, used: &[10, 0, 30, 0, 99], total_ticks: 100 }, 4096, ME, 6, |_| None);
        let names: Vec<&str> = apps.iter().map(|app| app.name.as_str()).collect();
        assert_eq!(names, ["firefox", "foot", "comm"], "kernel threads are left out");
        assert_eq!(apps[0].processes, 2);
        assert_eq!(apps[0].memory, 350 * 4096);
        assert_eq!(apps[0].cpu, 40.0);
        assert_eq!(apps[0].key, "/nix/store/x-firefox/lib/firefox/.firefox-wrapped");
        assert!(!apps[0].protected);
        assert_eq!(apps[2].key, "comm:comm");
        assert!(apps[2].protected, "another user's");
        assert_eq!(
            top(Look { processes: &processes, used: &[0; 5], total_ticks: 100 }, 4096, ME, 1, |_| None).len(),
            1
        );
    }

    #[test]
    fn nova_and_the_session_are_protected() {
        let processes = [
            process(100, 1, "/usr/bin/nova", 1000, 10),
            process(101, 100, "/nix/store/x/bin/.quickshell-wrapped", 1000, 10),
            process(7, 1, "/usr/bin/niri", 1000, 10),
        ];
        let apps = top(Look { processes: &processes, used: &[0; 3], total_ticks: 1 }, 4096, ME, 6, |_| None);
        assert!(apps.iter().all(|app| app.protected), "{apps:?}");
        assert!(end(&processes, NOVA_KEY, ME).unwrap_err().contains("can't be ended"));
        assert!(end(&processes, "/usr/bin/niri", ME).unwrap_err().contains("can't be ended"));
        assert!(end(&processes, "/usr/bin/gone", ME).unwrap_err().contains("no app"));
    }

    #[test]
    fn novad_and_its_ui_are_one_app_called_nova_always_listed() {
        let processes = [
            process(100, 1, "/usr/bin/nova", 1000, 10),
            process(101, 100, "/nix/store/x/bin/.quickshell-wrapped", 1000, 20),
            process(2, 1, "/usr/bin/foot", 1000, 300),
            process(3, 1, "/usr/bin/firefox", 1000, 400),
            process(4, 1, "/nix/store/x/bin/.quickshell-wrapped", 1000, 50),
        ];
        let apps = top(Look { processes: &processes, used: &[0; 5], total_ticks: 1 }, 4096, ME, 2, |_| None);
        let names: Vec<&str> = apps.iter().map(|app| app.name.as_str()).collect();
        assert_eq!(names, ["firefox", "foot", "Nova"], "Nova comes last when it doesn't rank");
        assert_eq!((apps[2].key.as_str(), apps[2].processes, apps[2].memory), (NOVA_KEY, 2, 30 * 4096));
        let apps = top(Look { processes: &processes, used: &[0; 5], total_ticks: 1 }, 4096, ME, 6, |_| None);
        let names: Vec<&str> = apps.iter().map(|app| app.name.as_str()).collect();
        assert_eq!(names, ["firefox", "foot", "quickshell", "Nova"], "another Quickshell isn't Nova");
    }

    #[test]
    fn listed_apps_show_proportional_memory_where_readable() {
        let processes = [
            process(1, 1, "/usr/bin/firefox", 1000, 400),
            process(2, 1, "/usr/bin/firefox", 1000, 400),
            process(3, 1, "/usr/bin/foot", 1000, 300),
        ];
        let apps = top(Look { processes: &processes, used: &[0; 3], total_ticks: 1 }, 4096, ME, 6, |pid| {
            (pid != 3).then_some(100 * 4096)
        });
        let shown: Vec<(&str, u64)> = apps.iter().map(|app| (app.name.as_str(), app.memory / 4096)).collect();
        assert_eq!(shown, [("foot", 300), ("firefox", 200)], "unreadable falls back to resident");
    }

    #[test]
    fn names_drop_nix_wrappers_and_deleted_programs() {
        let named = |program: &str| name(&process(1, 1, program, 1000, 1), ME);
        assert_eq!(named("/nix/store/x/bin/.Discord-wrapped"), "Discord");
        assert_eq!(named("/usr/lib/firefox/firefox (deleted)"), "firefox");
        assert_eq!(named(""), "comm");
    }

    #[test]
    fn ending_an_app_terminates_its_processes() {
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let me = Me::current();
        let processes = [Process {
            pid: child.id(),
            ppid: me.pid,
            name: "sleep".into(),
            program: Some("/test/app".into()),
            uid: Some(me.uid),
            ticks: 0,
            resident_pages: 1,
        }];
        let me = Me { pid: 0, ..me };
        assert_eq!(end(&processes, "/test/app", me), Ok(1));
        let status = child.wait().unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(libc::SIGTERM));
    }
}
