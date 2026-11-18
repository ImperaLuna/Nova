//! The machine and its user, for the quick settings header (feature
//! `quick-settings`): the host name, when it booted, the distribution from
//! os-release (its logo found in the icon theme), and what to call the
//! user. All of it comes from files, the user database and the config, read
//! when a client subscribes and again when the config's name changes.

use crate::icons::Icons;
use nova_protocol::{Host, OperatingSystem};

/// os-release's two homes: the first is the admin's, the second the
/// distribution's fallback (os-release(5)).
const OS_RELEASE: [&str; 2] = ["/etc/os-release", "/usr/lib/os-release"];
const HOSTNAME: &str = "/proc/sys/kernel/hostname";
/// os-release's `ID` when it has none (os-release(5)).
const DEFAULT_OS_ID: &str = "linux";
const DEFAULT_OS_NAME: &str = "Linux";

/// What the header shows now. `name` is what to call the user when the
/// config says (`[quick-settings] name`), else their login name.
pub fn read(name: Option<&str>, icons: &Icons) -> Host {
    let os_release = OS_RELEASE.iter().find_map(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
    let mut os = operating_system(&os_release);
    os.logo = os.logo.and_then(|logo| icons.find(&logo));
    Host {
        host: std::fs::read_to_string(HOSTNAME).map(|name| name.trim().to_string()).unwrap_or_default(),
        user: name.map_or_else(user_name, |name| name.trim().to_string()),
        booted_at: crate::procfs::boot_time().and_then(|booted| i64::try_from(booted).ok()).unwrap_or(0),
        os,
    }
}

/// `key`'s value in an os-release file's text, unquoted; an empty one is
/// none. `nova doctor` and `nova setup` read the distro with it too.
pub fn os_release_field(os_release: &str, key: &str) -> Option<String> {
    os_release.lines().find_map(|line| {
        let value = line.trim_start().strip_prefix(key)?.strip_prefix('=')?;
        Some(unquote(value.trim())).filter(|value| !value.is_empty())
    })
}

/// The distribution an os-release file describes, its logo by name.
fn operating_system(os_release: &str) -> OperatingSystem {
    let field = |key: &str| os_release_field(os_release, key);
    OperatingSystem {
        id: field("ID").unwrap_or_else(|| DEFAULT_OS_ID.to_string()),
        id_like: field("ID_LIKE").map(|like| like.split_whitespace().map(str::to_string).collect()).unwrap_or_default(),
        name: field("NAME").unwrap_or_else(|| DEFAULT_OS_NAME.to_string()),
        logo: field("LOGO"),
    }
}

/// An os-release value without its shell quoting: single or double quotes,
/// and backslash escapes inside double quotes.
fn unquote(value: &str) -> String {
    if let Some(inner) = value.strip_prefix('\'').and_then(|value| value.strip_suffix('\'')) {
        return inner.to_string();
    }
    let Some(inner) = value.strip_prefix('"').and_then(|value| value.strip_suffix('"')) else {
        return value.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            c => out.push(c),
        }
    }
    out
}

/// The user's login name, else `$USER`.
fn user_name() -> String {
    login_name().or_else(|| std::env::var("USER").ok()).unwrap_or_default()
}

/// This process's user's login name, from the user database (which may be
/// LDAP or systemd-userdb, not just /etc/passwd).
fn login_name() -> Option<String> {
    user_name_of(current_uid())
}

/// The user this process runs as.
pub(crate) fn current_uid() -> libc::uid_t {
    // SAFETY: getuid can't fail.
    unsafe { libc::getuid() }
}

/// Big enough for any real user or group entry; the lookups say ERANGE
/// otherwise.
const ENTRY_BUFFER_BYTES: usize = 16 * 1024;

/// User `uid`'s login name, from the user database.
pub(crate) fn user_name_of(uid: libc::uid_t) -> Option<String> {
    // SAFETY: getpwuid_r writes only within `entry` and `buffer` (the sizes
    // passed), and sets `found` to `entry` or null.
    database_name(
        |entry, buffer, size, found| unsafe { libc::getpwuid_r(uid, entry, buffer, size, found) },
        |entry: &libc::passwd| entry.pw_name,
    )
}

/// A name from a reentrant user or group database lookup: `lookup` fills
/// an entry, its strings in a buffer, and `name` picks the name from it.
fn database_name<Entry>(
    lookup: impl FnOnce(*mut Entry, *mut libc::c_char, usize, *mut *mut Entry) -> libc::c_int,
    name: impl FnOnce(&Entry) -> *mut libc::c_char,
) -> Option<String> {
    let mut buffer = vec![0u8; ENTRY_BUFFER_BYTES];
    // SAFETY: zeroed is a valid passwd or group (null pointers, zero ids);
    // it's only read after the lookup fills it.
    let mut entry: Entry = unsafe { std::mem::zeroed() };
    let mut found: *mut Entry = std::ptr::null_mut();
    let status = lookup(&mut entry, buffer.as_mut_ptr().cast(), buffer.len(), &mut found);
    let name = name(&entry);
    if status != 0 || found.is_null() || name.is_null() {
        return None;
    }
    // SAFETY: the lookup set it to a NUL-terminated string in `buffer`,
    // which is still alive.
    let name = unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy().into_owned();
    Some(name).filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_fields_are_unquoted() {
        let nixos = "ANSI_COLOR=\"0;38;2;126;186;228\"\nBUG_REPORT_URL=\"https://github.com/NixOS/nixpkgs/issues\"\nBUILD_ID=\"26.05.20261020.abc\"\nID=nixos\nID_LIKE=\"\"\nLOGO=\"nix-snowflake\"\nNAME=NixOS\nPRETTY_NAME=\"NixOS 26.05 (Yarara)\"\nVERSION_ID=\"26.05\"\n";
        let os = operating_system(nixos);
        assert_eq!(os.id, "nixos");
        assert!(os.id_like.is_empty(), "an empty ID_LIKE is none");
        assert_eq!(os.name, "NixOS");
        assert_eq!(os.logo.as_deref(), Some("nix-snowflake"));

        let manjaro = "NAME='Manjaro Linux'\nID=manjaro\nID_LIKE=\"arch\"\nPRETTY_NAME=\"Manjaro \\\"Linux\\\"\"\n";
        let os = operating_system(manjaro);
        assert_eq!((os.id.as_str(), os.name.as_str()), ("manjaro", "Manjaro Linux"));
        assert_eq!(os.id_like, ["arch"]);
        assert_eq!(os.logo, None);
        assert_eq!(unquote("\"Manjaro \\\"Linux\\\"\""), "Manjaro \"Linux\"");

        let ubuntu_like = "ID=pop\nID_LIKE=\"ubuntu debian\"\n";
        assert_eq!(operating_system(ubuntu_like).id_like, ["ubuntu", "debian"]);
    }

    #[test]
    fn missing_os_release_is_plain_linux() {
        let os = operating_system("");
        assert_eq!((os.id.as_str(), os.name.as_str()), (DEFAULT_OS_ID, DEFAULT_OS_NAME));
    }

    #[test]
    fn reads_this_machine() {
        let icons = Icons::new(crate::icons::HICOLOR.into(), &[], None);
        let host = read(None, &icons);
        assert!(host.booted_at > 0, "/proc/stat has btime");
        assert!(!host.user.is_empty(), "every user has a login name");
        assert_eq!(read(Some(" Katov "), &icons).user, "Katov", "the config's name wins");
        assert_eq!(host.os.logo, None, "no icon theme to find a logo in");
        assert!(!host.os.id.is_empty());
        if std::path::Path::new(HOSTNAME).exists() {
            assert!(!host.host.is_empty());
        }
    }
}
