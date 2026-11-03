//! Names of users and groups from the system's user database (which may be
//! LDAP or systemd-userdb, not just /etc/passwd and /etc/group), through
//! libc's reentrant lookups.

use std::ffi::{CStr, c_char};

/// Big enough for any real entry; the lookups say ERANGE otherwise.
const ENTRY_BUFFER: usize = 16 * 1024;

/// The login name of user `uid`, when the user database has one.
pub fn user_name(uid: libc::uid_t) -> Option<String> {
    let mut buffer = vec![0u8; ENTRY_BUFFER];
    // SAFETY: zeroed is a valid passwd (null pointers, zero ids); it's only
    // read after getpwuid_r fills it.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer is valid for the sizes passed, and `entry`'s
    // strings point into `buffer`, which outlives their use below.
    let status = unsafe { libc::getpwuid_r(uid, &mut entry, buffer.as_mut_ptr().cast(), buffer.len(), &mut found) };
    if status != 0 || found.is_null() {
        return None;
    }
    // SAFETY: getpwuid_r set it to a NUL-terminated string in `buffer`, or
    // left it null.
    unsafe { owned(entry.pw_name) }
}

/// The name of group `gid`, when the user database has one.
pub fn group_name(gid: libc::gid_t) -> Option<String> {
    let mut buffer = vec![0u8; ENTRY_BUFFER];
    // SAFETY: zeroed is a valid group (null pointers, zero id); it's only
    // read after getgrgid_r fills it.
    let mut entry: libc::group = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::group = std::ptr::null_mut();
    // SAFETY: as in `user_name`.
    let status = unsafe { libc::getgrgid_r(gid, &mut entry, buffer.as_mut_ptr().cast(), buffer.len(), &mut found) };
    if status != 0 || found.is_null() {
        return None;
    }
    // SAFETY: as in `user_name`.
    unsafe { owned(entry.gr_name) }
}

/// The user this process runs as.
pub fn current_uid() -> libc::uid_t {
    // SAFETY: getuid has no preconditions and can't fail.
    unsafe { libc::getuid() }
}

/// `name` copied out, unless it's null or empty.
///
/// # Safety
/// `name` is null or a NUL-terminated string valid for the call.
unsafe fn owned(name: *const c_char) -> Option<String> {
    if name.is_null() {
        return None;
    }
    // SAFETY: the caller's promise.
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned();
    Some(name).filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_has_a_name_and_a_group() {
        assert_eq!(user_name(0).as_deref(), Some("root"));
        assert!(group_name(0).is_some_and(|name| !name.is_empty()));
    }

    #[test]
    fn an_unknown_id_has_no_name() {
        assert_eq!(user_name(4_000_000_000), None);
        assert_eq!(group_name(4_000_000_000), None);
    }
}
