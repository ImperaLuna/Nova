use nova_base::lock;
use std::collections::VecDeque;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStderr, Command, ExitStatus, Output, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

const MAX_ERROR_BYTES: usize = 4096;
pub const DRAIN_TIMEOUT: Duration = Duration::from_millis(500);

pub fn wait_within(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    if let Some(status) = child.try_wait()? {
        return Ok(Some(status));
    }
    let exited = pidfd(child)?;
    let deadline = Instant::now() + timeout;
    let mut poll = libc::pollfd { fd: exited.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let millis = libc::c_int::try_from(left.as_micros().div_ceil(1000)).unwrap_or(libc::c_int::MAX);
        // SAFETY: `poll` is one valid pollfd for the whole call.
        let ready = unsafe { libc::poll(&mut poll, 1, millis) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if ready > 0 || left.is_zero() {
            return child.try_wait();
        }
    }
}

fn pidfd(child: &Child) -> std::io::Result<OwnedFd> {
    let pid = libc::pid_t::try_from(child.id()).map_err(std::io::Error::other)?;
    // SAFETY: pidfd_open takes a pid and flags, and no pointers.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let fd = libc::c_int::try_from(fd).map_err(std::io::Error::other)?;
    // SAFETY: the kernel just opened `fd` for us, and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

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
    let status = match wait_within(&mut child, timeout) {
        Ok(Some(status)) => status,
        waited => {
            kill_and_reap(&mut child);
            let message = format!("no answer in {} s", timeout.as_secs_f32());
            return Err(waited.err().unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, message)));
        }
    };
    let stdout = printed
        .recv_timeout(DRAIN_TIMEOUT)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "its output stayed open after it exited"))?;
    Ok(Output { status, stdout, stderr: errors.text().into_bytes() })
}

pub fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

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

pub fn die_with_novad(command: &mut Command, signal: libc::c_int) {
    let novad = std::process::id();
    // SAFETY: the closure runs between fork and exec and only calls prctl
    // and getppid, which take no pointers and are async-signal-safe.
    unsafe {
        command.pre_exec(move || {
            // The death signal comes when the thread that forked the child ends, not the process:
            // https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html
            if libc::prctl(libc::PR_SET_PDEATHSIG, signal) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid().cast_unsigned() != novad {
                return Err(std::io::Error::other("novad went away"));
            }
            Ok(())
        });
    }
}

pub fn watch(command: &mut Command, exited: impl FnOnce(Exited) + Send + 'static) -> std::io::Result<u32> {
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    die_with_novad(command, libc::SIGTERM);
    let mut child = command.spawn()?;
    let pid = child.id();
    let errors = drain(child.stderr.take());
    std::thread::spawn(move || {
        let waited = wait_for_exit(pid);
        exited(Exited { child, errors, waited });
    });
    Ok(pid)
}

pub struct Exited {
    child: Child,
    errors: ErrorOutput,
    waited: std::io::Result<()>,
}

impl Exited {
    pub fn reap(mut self) -> String {
        if let Err(e) = self.waited {
            kill_and_reap(&mut self.child);
            return format!("couldn't wait for it without reaping it: {e}");
        }
        let status = self.child.wait().map_or_else(|e| e.to_string(), |status| status.to_string());
        let said = self.errors.text();
        match said.trim() {
            "" => status,
            said => format!("{status}: {said}"),
        }
    }
}

pub fn terminate(pid: u32) {
    // SAFETY: kill only sends a signal.
    unsafe { libc::kill(pid.cast_signed(), libc::SIGTERM) };
}

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

pub struct ErrorOutput {
    kept: Arc<Mutex<VecDeque<u8>>>,
    done: mpsc::Receiver<()>,
}

impl ErrorOutput {
    pub fn text(self) -> String {
        let _ = self.done.recv_timeout(DRAIN_TIMEOUT);
        String::from_utf8_lossy(lock(&self.kept).make_contiguous()).into_owned()
    }
}

pub fn stdout_within(command: Command, timeout: Duration) -> std::io::Result<Vec<u8>> {
    let output = output_within(command, timeout)?;
    match output.status.success() {
        true => Ok(output.stdout),
        false => Err(std::io::Error::other(describe_exit(output.status))),
    }
}

pub fn describe_exit(status: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit code {code}"),
        (None, Some(signal)) => format!("killed by signal {signal}"),
        (None, None) => "unknown exit".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_that_hangs_is_killed() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let started = Instant::now();
        let error = stdout_within(command, Duration::from_millis(200)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_command_that_closes_its_output_but_hangs_is_killed() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec >&-; sleep 30"]);
        let started = Instant::now();
        let error = stdout_within(command, Duration::from_millis(200)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_quick_command_gives_its_output() {
        let mut command = Command::new("echo");
        command.arg("hello");
        assert_eq!(stdout_within(command, Duration::from_secs(5)).unwrap(), b"hello\n");
    }

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
    fn waiting_ends_when_the_child_exits_not_at_the_deadline() {
        let timeout = Duration::from_secs(30);
        let mut child = Command::new("sleep").arg("0.1").spawn().unwrap();
        let started = Instant::now();
        assert!(wait_within(&mut child, timeout).unwrap().is_some_and(|status| status.success()));
        assert!(started.elapsed() < timeout / 2, "woken by the exit: {:?}", started.elapsed());
    }

    #[test]
    fn waiting_for_exit_leaves_the_child_unreaped() {
        let mut child = Command::new("sh").args(["-c", "exit 3"]).spawn().unwrap();
        wait_for_exit(child.id()).unwrap();
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", child.id())).unwrap();
        assert_eq!(stat.rsplit_once(") ").unwrap().1.chars().next(), Some('Z'));
        assert_eq!(child.wait().unwrap().code(), Some(3));
    }

    #[test]
    fn error_output_keeps_the_end_and_stops_waiting_for_a_held_pipe() {
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
