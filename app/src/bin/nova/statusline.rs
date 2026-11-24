use nova_ai_usage_api::Command;
use nova_carrier::encode;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

const MAX_INPUT_BYTES: u64 = 64 * 1024;
const SOCKET_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(clap::Args)]
pub struct StatuslineArgs {
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
        let _ = stdin.write_all(&input);
    }
    let status = child.wait().map_err(|e| format!("{program}: {e}"))?;
    Ok(std::process::ExitCode::from(status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1)))
}

fn report(socket: &Path, command: Command) {
    let Ok(line) = encode(nova_carrier::Request { id: None, command }) else { return };
    let socket = socket.to_owned();
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        if let Ok(mut stream) = UnixStream::connect(socket) {
            let _ = stream.set_write_timeout(Some(SOCKET_TIMEOUT));
            let _ = stream.write_all(line.as_bytes());
        }
        let _ = done.send(());
    });
    let _ = finished.recv_timeout(SOCKET_TIMEOUT);
}
