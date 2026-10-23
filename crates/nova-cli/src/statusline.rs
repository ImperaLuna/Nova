//! `nova claude-statusline`: Claude Code's status line command. Claude Code
//! pipes it a JSON description of the session, including the plan's rate
//! limits; this sends only those limits to novad for the AI usage widget.
//!
//! It must never break Claude Code's status line: when novad isn't running
//! or the feature is off, the limits are dropped without a word. A status
//! line command the user already had can run after it, on the same input.

use nova_protocol::{Command, Request, encode};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

/// Claude Code's input is a few KiB; anything past this isn't read.
const MAX_INPUT_BYTES: u64 = 64 * 1024;
/// Claude Code waits on the status line, so novad gets little time.
const SOCKET_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(clap::Args)]
pub struct StatuslineArgs {
    /// Your own status line command, run after with the same input; its
    /// output is the status line. Put it after `--`.
    #[arg(last = true)]
    command: Vec<String>,
}

pub fn run(args: &StatuslineArgs, socket: &Path) -> Result<std::process::ExitCode, String> {
    let mut input = Vec::new();
    std::io::stdin().take(MAX_INPUT_BYTES).read_to_end(&mut input).map_err(|e| format!("can't read stdin: {e}"))?;
    if let Ok(json) = serde_json::from_slice(&input) {
        let limits = novad::claude_limits_from_statusline(&json);
        if !limits.is_empty() {
            report(socket, Command::ReportClaudeUsage { limits });
        }
    }
    let Some((program, program_args)) = args.command.split_first() else {
        return Ok(std::process::ExitCode::SUCCESS);
    };
    let mut child = std::process::Command::new(program)
        .args(program_args)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("can't run {program}: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A command that doesn't read its input closes the pipe early.
        let _ = stdin.write_all(&input);
    }
    let status = child.wait().map_err(|e| format!("{program}: {e}"))?;
    Ok(std::process::ExitCode::from(status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1)))
}

/// Sends `command` without waiting for an answer; failures are ignored.
fn report(socket: &Path, command: Command) {
    let Ok(mut stream) = UnixStream::connect(socket) else { return };
    let _ = stream.set_write_timeout(Some(SOCKET_TIMEOUT));
    if let Ok(line) = encode(Request { id: None, command }) {
        let _ = stream.write_all(line.as_bytes());
    }
}
