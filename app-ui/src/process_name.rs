use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

#[cfg(target_os = "linux")]
// The name `top` and btop show: https://man7.org/linux/man-pages/man2/PR_SET_NAME.2const.html
const PR_SET_NAME: std::ffi::c_int = 15;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn prctl(option: std::ffi::c_int, ...) -> std::ffi::c_int;
}

pub fn follow_command_line() {
    let Some(name) = std::env::args_os().next().as_deref().and_then(shown_name) else { return };
    #[cfg(target_os = "linux")]
    // SAFETY: PR_SET_NAME reads a NUL-terminated string, which `name` is for
    // the whole call; the kernel copies at most 16 bytes of it.
    if unsafe { prctl(PR_SET_NAME, name.as_ptr()) } == -1 {
        eprintln!("WARN nova-ui: kept the process name {:?}: {}", name, std::io::Error::last_os_error());
    }
}

fn shown_name(arg0: &OsStr) -> Option<CString> {
    let name = Path::new(arg0).file_name()?;
    CString::new(name.as_bytes()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_is_the_command_line_s_first_word() {
        let name = |arg0: &str| shown_name(OsStr::new(arg0)).map(|name| name.into_string().unwrap());
        assert_eq!(name("NovaUI").as_deref(), Some("NovaUI"), "as novad starts it");
        assert_eq!(name("/nix/store/x-nova-ui/bin/nova-ui").as_deref(), Some("nova-ui"), "started by hand");
        assert_eq!(name("").as_deref(), None);
    }
}
