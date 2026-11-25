use crate::prompt::{Secret, Verdict, wipe};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs::{DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const SERVICE: &CStr = c"nova-lock";
const SERVICE_FILE: &str = "nova-lock";
const SERVICE_CONFIG: &str = include_str!("../pam/nova-lock");
const CONFIG_FOLDER: &str = "nova-ui/pam";
const RUNTIME_DIR_VAR: &str = "XDG_RUNTIME_DIR";
const PRIVATE_FOLDER: u32 = 0o700;
const PRIVATE_FILE: u32 = 0o600;

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_AUTH_ERR: c_int = 7;
const PAM_MAXTRIES: c_int = 11;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;

#[repr(C)]
struct Message {
    style: c_int,
    text: *const c_char,
}

#[repr(C)]
struct Response {
    text: *mut c_char,
    code: c_int,
}

#[repr(C)]
struct Conversation {
    answer: unsafe extern "C" fn(c_int, *const *const Message, *mut *mut Response, *mut c_void) -> c_int,
    password: *mut c_void,
}

#[repr(C)]
struct Handle {
    _opaque: [u8; 0],
}

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start_confdir(
        service: *const c_char,
        user: *const c_char,
        conversation: *const Conversation,
        config_folder: *const c_char,
        handle: *mut *mut Handle,
    ) -> c_int;
    fn pam_authenticate(handle: *mut Handle, flags: c_int) -> c_int;
    fn pam_end(handle: *mut Handle, status: c_int) -> c_int;
}

unsafe extern "C" {
    fn calloc(count: usize, size: usize) -> *mut c_void;
    fn strdup(text: *const c_char) -> *mut c_char;
    fn free(memory: *mut c_void);
}

pub fn check(user: &str, password: Secret) -> Verdict {
    let folder = match std::env::var_os(RUNTIME_DIR_VAR) {
        Some(runtime) => PathBuf::from(runtime).join(CONFIG_FOLDER),
        None => return Verdict::Unchecked(format!("{RUNTIME_DIR_VAR} isn't set, so PAM's service file has no place")),
    };
    if let Err(error) = write_service(&folder) {
        return Verdict::Unchecked(format!("couldn't write PAM's service file in {}: {error}", folder.display()));
    }
    let (Ok(user), Ok(folder)) = (CString::new(user), CString::new(folder.as_os_str().as_bytes())) else {
        return Verdict::Unchecked("the user's name or the runtime directory holds a NUL byte".into());
    };
    let password = match CString::from_vec_with_nul(password.to_c_bytes()) {
        Ok(password) => password,
        Err(refused) => {
            wipe(refused.into_bytes());
            return Verdict::Unchecked("the password holds a NUL byte".into());
        }
    };
    let result = authenticate(&user, &password, &folder);
    wipe(password.into_bytes());
    match result {
        PAM_SUCCESS => Verdict::Right,
        PAM_AUTH_ERR => Verdict::Wrong,
        PAM_MAXTRIES => Verdict::TooManyTries,
        code => Verdict::Unchecked(format!("PAM answered {code}")),
    }
}

fn write_service(folder: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(PRIVATE_FOLDER).create(folder)?;
    let new = folder.join(format!(".{SERVICE_FILE}.{}", std::process::id()));
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(PRIVATE_FILE).open(&new)?;
    file.write_all(SERVICE_CONFIG.as_bytes())?;
    drop(file);
    std::fs::rename(&new, folder.join(SERVICE_FILE))
}

fn authenticate(user: &CStr, password: &CStr, config_folder: &CStr) -> c_int {
    let conversation = Conversation { answer, password: password.as_ptr().cast_mut().cast() };
    let mut handle: *mut Handle = std::ptr::null_mut();
    // SAFETY: every pointer is to a NUL-terminated string or a struct that
    // lives until `pam_end` below, and `handle` is a valid place for PAM to
    // store its handle. PAM keeps `conversation` only for this transaction.
    let started = unsafe {
        pam_start_confdir(
            SERVICE.as_ptr(),
            user.as_ptr(),
            &raw const conversation,
            config_folder.as_ptr(),
            &raw mut handle,
        )
    };
    if started != PAM_SUCCESS || handle.is_null() {
        return if started == PAM_SUCCESS { PAM_CONV_ERR } else { started };
    }
    // SAFETY: `handle` came from a successful `pam_start_confdir` and is
    // ended exactly once, here.
    unsafe {
        let result = pam_authenticate(handle, 0);
        pam_end(handle, result);
        result
    }
}

unsafe extern "C" fn answer(
    count: c_int,
    messages: *const *const Message,
    responses: *mut *mut Response,
    password: *mut c_void,
) -> c_int {
    let Ok(count) = usize::try_from(count) else { return PAM_CONV_ERR };
    if count == 0 || messages.is_null() || responses.is_null() || password.is_null() {
        return PAM_CONV_ERR;
    }
    // SAFETY: `calloc` gives zeroed memory for `count` responses or null.
    let answers = unsafe { calloc(count, size_of::<Response>()) }.cast::<Response>();
    if answers.is_null() {
        return PAM_BUF_ERR;
    }
    for index in 0..count {
        // SAFETY: PAM passes `count` message pointers; `answers` holds
        // `count` responses; `password` is a NUL-terminated string.
        let copied = unsafe {
            let message = *messages.add(index);
            if message.is_null() || (*message).style != PAM_PROMPT_ECHO_OFF {
                continue;
            }
            let copy = strdup(password.cast());
            (*answers.add(index)).text = copy;
            !copy.is_null()
        };
        if !copied {
            // SAFETY: the answers so far are this function's own.
            unsafe { free_answers(answers, index) };
            return PAM_BUF_ERR;
        }
    }
    // SAFETY: `responses` is PAM's place for the answers, which it frees.
    unsafe { *responses = answers };
    PAM_SUCCESS
}

unsafe fn free_answers(answers: *mut Response, filled: usize) {
    for index in 0..filled {
        // SAFETY: as the function's contract says: each text is null or a
        // NUL-terminated copy of the password, overwritten up to its NUL
        // before it is freed; `free(null)` is fine.
        unsafe {
            let text = (*answers.add(index)).text;
            let mut byte = text;
            while !byte.is_null() && *byte != 0 {
                byte.write_volatile(0);
                byte = byte.add(1);
            }
            free(text.cast());
        }
    }
    // SAFETY: `answers` came from `calloc`.
    unsafe { free(answers.cast()) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_service_file_is_written_private_and_whole() {
        let folder = std::env::temp_dir().join(format!("nova-lock-pam-{}", std::process::id())).join("pam");
        write_service(&folder).unwrap();
        std::fs::write(folder.join(SERVICE_FILE), "auth sufficient pam_permit.so\n").unwrap();
        write_service(&folder).unwrap();

        let file = folder.join(SERVICE_FILE);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), SERVICE_CONFIG);
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, PRIVATE_FILE);
        assert_eq!(std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777, PRIVATE_FOLDER);
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1, "the temporary file is gone");
        std::fs::remove_dir_all(folder.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_service_checks_the_login_password() {
        assert!(SERVICE_CONFIG.lines().any(|line| line.trim() == "auth required pam_unix.so"));
        assert_eq!(SERVICE.to_str().unwrap(), SERVICE_FILE);
    }
}
