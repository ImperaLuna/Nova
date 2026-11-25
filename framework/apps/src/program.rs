pub const XEMBED_TRAY_BRIDGE: &str = "xembedsniproxy";

const GENERIC_PROGRAMS: &[&str] = &["electron", "xdg-dbus-proxy", "bwrap", "python", "java", XEMBED_TRAY_BRIDGE];

pub fn program_name(program: &str) -> Option<String> {
    let name = program.trim().rsplit('/').next()?;
    let name = name.strip_prefix('.').unwrap_or(name);
    // The kernel cuts `comm` to 15 bytes, so Nix's `.<name>-wrapped` may read `.<name>-wrappe`:
    // https://man7.org/linux/man-pages/man5/proc_pid_comm.5.html
    let name = name.strip_suffix("-wrapped").or_else(|| name.strip_suffix("-wrappe")).unwrap_or(name);
    let unversioned = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.').to_lowercase();
    (!name.is_empty() && !GENERIC_PROGRAMS.contains(&unversioned.as_str())).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nix_wrappers_and_runtimes() {
        assert_eq!(program_name("/nix/store/c42f-discord/opt/Discord/.Discord-wrapped").as_deref(), Some("Discord"));
        assert_eq!(program_name(".ferdium-wrappe\n").as_deref(), Some("ferdium"));
        assert_eq!(program_name("claude-desktop").as_deref(), Some("claude-desktop"));
        assert_eq!(program_name("/usr/lib/electron33/electron"), None);
        assert_eq!(program_name("Electron"), None);
        assert_eq!(program_name("/nix/store/xahx-python3-3.14.8/bin/python3.14"), None);
    }
}
