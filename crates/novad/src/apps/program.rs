//! Which app a running program is, from its file: Nix wraps programs as
//! `.<name>-wrapped`, and shared runtimes (Electron, Python) say nothing
//! about the app they run. The tray and app streams' icons look apps up by
//! these names (`Launcher::find_app`).

/// Program names that say nothing about the app: shared runtimes and
/// sandbox helpers that run many apps, and the bridge for old X11 tray
/// icons.
const GENERIC_PROGRAMS: &[&str] = &["electron", "xdg-dbus-proxy", "bwrap", "python", "java", crate::tray::BRIDGE];

/// The app a program path or name stands for, unless it's a shared
/// runtime (whatever its version: `python3.14`). Nix wraps programs as
/// `.<name>-wrapped` (cut to `.<name>-wrappe` in the kernel's 15-character
/// `comm`).
pub fn program_name(program: &str) -> Option<String> {
    let name = program.trim().rsplit('/').next()?;
    let name = name.strip_prefix('.').unwrap_or(name);
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
