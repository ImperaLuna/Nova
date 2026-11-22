//! `--replay`: a fake novad inside this process that replays recorded
//! events, so a feature's UI runs where the real novad can't (a cloud
//! session without the desktop's services).

use nova_ui_client::fake::{FakeDaemon, Recorded};
use std::path::PathBuf;

const RUNTIME_DIR_VAR: &str = "XDG_RUNTIME_DIR";

/// Starts the fake novad on a socket of this process's own when there are
/// `folders` to replay. It runs until the value returned is dropped.
pub fn start(folders: &[PathBuf]) -> Result<Option<(PathBuf, FakeDaemon)>, String> {
    if folders.is_empty() {
        return Ok(None);
    }
    let mut recorded = Recorded::new();
    for folder in folders {
        recorded = recorded.read(folder)?;
    }
    // In the session's own folder when there is one, under a name no other
    // run has.
    let runtime_dir = std::env::var_os(RUNTIME_DIR_VAR).filter(|folder| !folder.is_empty());
    let folder = runtime_dir.map_or_else(std::env::temp_dir, PathBuf::from);
    let socket = folder.join(format!("nova-ui-replay-{}.sock", std::process::id()));
    let problem = |e: std::io::Error| format!("can't start the fake novad at {}: {e}", socket.display());
    let novad = FakeDaemon::start(&socket, recorded).map_err(problem)?;
    Ok(Some((socket, novad)))
}
