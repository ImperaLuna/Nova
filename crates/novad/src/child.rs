//! Small helpers for the short-lived programs novad runs and the ones that
//! live as long as it does: waiting with a deadline, reading error output
//! without blocking on it, waiting for an exit without reaping, and tying a
//! child's life to novad's. Each caller still decides what a slow child
//! means for it (killed, left to finish, or logged); long-running
//! supervision stays in `supervisor`, `recording` and `x11_tray`.

use crate::lock;
use std::collections::VecDeque;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStderr, Command, ExitStatus, Output, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// How often a running child is checked on until it exits.
const EXIT_POLL: Duration = Duration::from_millis(10);
/// The most of a child's error output kept for its error message.
const MAX_ERROR_BYTES: usize = 4096;
/// How long error output is read for once the child has exited: a process
/// it left behind may hold the pipe open.
pub const DRAIN_TIMEOUT: Duration = Duration::from_millis(500);

/// `child`'s exit status if it exits within `timeout`, else `None` with the
/// child still running (and unreaped).
pub fn wait_within(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(EXIT_POLL);
    }
}

/// Runs `command` to the end and returns its exit status, its output and
/// (the end of) its error output, like [`Command::output`] but killed and
/// reaped when it hasn't exited within `timeout`.
pub fn output_within(mut command: Command, timeout: Duration) -> std::io::Result<Output> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("no stdout pipe"))?;
    let (done, printed) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = stdout.read_to_end(&mut text);
        let _ = done.send(text);
    });
    let errors = drain(child.stderr.take());
    let Some(status) = wait_within(&mut child, timeout)? else {
        kill_and_reap(&mut child);
        let message = format!("no answer in {} s", timeout.as_secs_f32());
        return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, message));
    };
    // Something it left running may hold the pipe open.
    let stdout = printed.recv_timeout(DRAIN_TIMEOUT).unwrap_or_default();
    Ok(Output { status, stdout, stderr: errors.text().into_bytes() })
}

/// Kills `child` and reaps it. It's still unreaped, so the pid is still its.
pub fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Waits until child `pid` exits, leaving it unreaped (a zombie), so its pid
/// can't be reused yet.
pub fn wait_for_exit(pid: u32) -> std::io::Result<()> {
    loop {
        // SAFETY: siginfo_t is plain data, and waitid only writes into it.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: `info` is a valid siginfo_t for the whole call.
        if unsafe { libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOWAIT) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// Has `command`'s child get `signal` when novad goes away, however it goes,
/// so it's never left running on its own. The kernel sends the signal when
/// the thread that spawned the child ends, so spawn it from a thread that
/// lives as long as the child should.
pub fn die_with_novad(command: &mut Command, signal: libc::c_int) {
    let novad = std::process::id();
    // SAFETY: the closure runs between fork and exec and only calls prctl
    // and getppid, which take no pointers and are async-signal-safe.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, signal) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            // novad went away before that, so the signal never comes.
            if libc::getppid().cast_unsigned() != novad {
                return Err(std::io::Error::other("novad went away"));
            }
            Ok(())
        });
    }
}

/// Reads a child's error output to the end on a thread of its own, so the
/// child never blocks on a full pipe, keeping the last [`MAX_ERROR_BYTES`]
/// for an error message.
pub fn drain(stderr: Option<ChildStderr>) -> ErrorOutput {
    let kept = Arc::new(Mutex::new(VecDeque::new()));
    let (closed, done) = mpsc::channel();
    let reading = Arc::clone(&kept);
    std::thread::spawn(move || {
        let _closed = closed;
        let Some(mut stderr) = stderr else {
            return;
        };
        let mut buffer = [0; 4096];
        while let Ok(read) = stderr.read(&mut buffer) {
            if read == 0 {
                break;
            }
            let mut kept = lock(&reading);
            kept.extend(&buffer[..read]);
            let extra = kept.len().saturating_sub(MAX_ERROR_BYTES);
            kept.drain(..extra);
        }
    });
    ErrorOutput { kept, done }
}

/// The error output [`drain`] is reading.
pub struct ErrorOutput {
    kept: Arc<Mutex<VecDeque<u8>>>,
    /// Disconnects once the pipe closes.
    done: mpsc::Receiver<()>,
}

impl ErrorOutput {
    /// What the child said, once the pipe closes or after
    /// [`DRAIN_TIMEOUT`], whichever is first.
    pub fn text(self) -> String {
        let _ = self.done.recv_timeout(DRAIN_TIMEOUT);
        String::from_utf8_lossy(lock(&self.kept).make_contiguous()).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_gives_up_at_the_deadline_and_leaves_the_child_running() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        assert!(wait_within(&mut child, Duration::from_millis(50)).unwrap().is_none());
        assert!(child.try_wait().unwrap().is_none(), "still running");
        kill_and_reap(&mut child);
        let mut quick = Command::new("true").spawn().unwrap();
        assert!(wait_within(&mut quick, Duration::from_secs(5)).unwrap().is_some_and(|status| status.success()));
    }

    #[test]
    fn waiting_for_exit_leaves_the_child_unreaped() {
        let mut child = Command::new("sh").args(["-c", "exit 3"]).spawn().unwrap();
        wait_for_exit(child.id()).unwrap();
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", child.id())).unwrap();
        // The state follows the command name in parentheses: Z is a zombie.
        assert_eq!(stat.rsplit_once(") ").unwrap().1.chars().next(), Some('Z'));
        assert_eq!(child.wait().unwrap().code(), Some(3));
    }

    #[test]
    fn error_output_keeps_the_end_and_stops_waiting_for_a_held_pipe() {
        // Prints more than is kept, then leaves a process holding the pipe.
        let script = format!("head -c {} /dev/zero | tr '\\0' a; echo the end >&2; sleep 30 &", MAX_ERROR_BYTES * 2);
        let mut command = Command::new("sh");
        command.args(["-c", &format!("({script}) 1>&2")]).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let errors = drain(child.stderr.take());
        child.wait().unwrap();
        let started = Instant::now();
        let text = errors.text();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(text.len(), MAX_ERROR_BYTES);
        assert!(text.ends_with("the end\n"), "{:?}", &text[text.len() - 20..]);
    }
}
