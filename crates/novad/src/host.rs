//! The machine and its user, for the quick settings header (feature
//! `quick-settings`): the host name, when it booted, the distribution from
//! os-release, and what to call the user. All of it comes from files, the
//! user database and the config, read when a client subscribes and again
//! when the config's name changes.

use nova_protocol::{Host, OperatingSystem};

/// os-release's two homes: the first is the admin's, the second the
/// distribution's fallback (os-release(5)).
const OS_RELEASE: [&str; 2] = ["/etc/os-release", "/usr/lib/os-release"];
const HOSTNAME: &str = "/proc/sys/kernel/hostname";
const STAT: &str = "/proc/stat";
/// os-release's `ID` when it has none (os-release(5)).
const DEFAULT_OS_ID: &str = "linux";
const DEFAULT_OS_NAME: &str = "Linux";

/// What the header shows now. `name` is what to call the user when the
/// config says (`[quick-settings] name`), else their account's first name.
pub fn read(name: Option<&str>) -> Host {
    let os_release = OS_RELEASE.iter().find_map(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
    Host {
        host: std::fs::read_to_string(HOSTNAME).map(|name| name.trim().to_string()).unwrap_or_default(),
        user: name.map_or_else(user_name, |name| name.trim().to_string()),
        booted_at: std::fs::read_to_string(STAT).ok().and_then(|stat| boot_time(&stat)).unwrap_or(0),
        os: operating_system(&os_release),
    }
}

/// `btime` from `/proc/stat`: when the machine booted, in Unix seconds.
fn boot_time(stat: &str) -> Option<i64> {
    stat.lines().find_map(|line| line.strip_prefix("btime ")?.trim().parse().ok())
}

/// The distribution an os-release file describes.
fn operating_system(os_release: &str) -> OperatingSystem {
    let field = |key: &str| {
        os_release.lines().find_map(|line| {
            let value = line.strip_prefix(key)?.strip_prefix('=')?;
            Some(unquote(value.trim())).filter(|value| !value.is_empty())
        })
    };
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

/// The first name from the user's full name (the GECOS field), else their
/// login name, else `$USER`.
fn user_name() -> String {
    let (login, gecos) = account();
    first_name(&gecos).or(login).or_else(|| std::env::var("USER").ok()).unwrap_or_default()
}

/// The GECOS field's full name, up to its first comma, first word only.
fn first_name(gecos: &str) -> Option<String> {
    gecos.split(',').next()?.split_whitespace().next().map(str::to_string)
}

/// This process's user's login name and GECOS field, from the user
/// database (which may be LDAP or systemd-userdb, not just /etc/passwd).
fn account() -> (Option<String>, String) {
    // Big enough for any real entry; getpwuid_r says ERANGE otherwise.
    let mut buffer = vec![0u8; 16 * 1024];
    // SAFETY: zeroed is a valid passwd (null pointers, zero ids); it's only
    // read after getpwuid_r fills it.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer is valid for the sizes passed, and `entry`'s
    // strings point into `buffer`, which outlives their use below.
    let status =
        unsafe { libc::getpwuid_r(libc::getuid(), &mut entry, buffer.as_mut_ptr().cast(), buffer.len(), &mut found) };
    if status != 0 || found.is_null() {
        return (None, String::new());
    }
    let text = |pointer: *const libc::c_char| {
        // SAFETY: getpwuid_r set it to null or a NUL-terminated string in `buffer`.
        (!pointer.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr(pointer) }.to_string_lossy().into_owned())
    };
    (text(entry.pw_name).filter(|name| !name.is_empty()), text(entry.pw_gecos).unwrap_or_default())
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
    fn first_name_comes_from_the_full_name() {
        assert_eq!(first_name("Luna Impera,,,"), Some("Luna".into()));
        assert_eq!(first_name(",room 4"), None);
        assert_eq!(first_name(""), None);
    }

    #[test]
    fn boot_time_from_proc_stat() {
        let stat = "cpu  1 2 3 4 5 6 7 0 0 0\nintr 0\nctxt 9\nbtime 1793000000\nprocesses 1\n";
        assert_eq!(boot_time(stat), Some(1_793_000_000));
        assert_eq!(boot_time("cpu 1 2 3\n"), None);
    }

    #[test]
    fn reads_this_machine() {
        let host = read(None);
        assert!(host.booted_at > 0, "/proc/stat has btime");
        assert!(!host.user.is_empty(), "every user has a name or a login");
        assert_eq!(read(Some(" Katov ")).user, "Katov", "the config's name wins");
        assert!(!host.os.id.is_empty());
        if std::path::Path::new(HOSTNAME).exists() {
            assert!(!host.host.is_empty());
        }
    }
}
