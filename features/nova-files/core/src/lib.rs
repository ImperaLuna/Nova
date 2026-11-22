//! The standalone Nova Files app (feature `nova-files`, its provider view
//! in `nova-nova-files-api`): it answers the launcher's Files mode
//! (`launcher.rs`), finding files by name and opening them or showing them
//! in their folder. novad's wiring runs it as a module and plugs it into
//! the launcher as that view's provider, so neither feature names the
//! other's code.
//!
//! Nova Files serves `io.github.ImperaLuna.NovaFiles.Search1` at its app id,
//! which the bus starts it for (D-Bus activation) and which it leaves after
//! a few idle minutes. `Search` returns at once and its stages come as
//! `Found` signals sent only to the caller, until one is `done`; a search
//! waits [`STAGES_WAIT`] for that, then answers with the stages it has.
//! Starting a search cancels the one before if it's still going.
//! `Complete` answers at once with the entries a typed path (`~/Doc`) can
//! complete to, read from its folder as it is now. `Open`
//! opens a file in its default app or a folder in a Nova Files window, and
//! `ShowInFolder` opens a window on the folder that holds a path.
//!
//! In the feature's `launcher` mode Nova Files is only that search service,
//! with no app the user picked to browse in, so folders open, and items show
//! in their folder, in the user's own file manager instead
//! (`org.freedesktop.FileManager1`).
//!
//! One connection, kept while the feature runs, with one thread reading the
//! signals and handing each stage to the search it belongs to. The shared
//! MIME database (`nova-mime`) types what's found, for its icon.

mod client;
mod launcher;

use client::Client;
use nova_log::Logger;
use nova_mime::Mime;
use nova_session_bus::Bus;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const ID: &str = "nova-files";
/// The feature's mode with only the search service, no Nova Files windows.
pub const LAUNCHER_MODE: &str = "launcher";
const LOG_SOURCE: &str = "nova-files";
/// How long a search waits for its last stage. Nova Files answers a warm
/// search of a real Home in about 30 ms; past this, what came is shown.
pub const STAGES_WAIT: Duration = Duration::from_millis(100);
/// How long a call (and connecting) may take. The first call starts Nova
/// Files, so it gets longer than a search waits for stages.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// Where the feature finds Nova Files and what it needs to show its finds.
#[derive(Clone)]
pub struct Sources {
    pub bus: Bus,
    /// The XDG data folders, for the shared MIME database.
    pub data_dirs: Vec<PathBuf>,
    /// The user's Home, for spelling paths in it from `~`.
    pub home: Option<PathBuf>,
}

/// The connection to Nova Files, while the feature runs.
pub struct NovaFiles {
    client: Result<Client, String>,
    mime: Arc<Mime>,
    home: Option<PathBuf>,
    /// Whether folders open in Nova Files' own windows: every mode but
    /// [`LAUNCHER_MODE`].
    own_windows: bool,
}

impl NovaFiles {
    /// The paths Nova Files finds for `query`, best first, at most `limit`
    /// per stage.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<PathBuf>, String> {
        self.client()?.search(query, limit, STAGES_WAIT)
    }

    /// The paths `typed` (a path as typed, from `~` or `/`) completes to,
    /// at most `limit`.
    pub fn complete(&self, typed: &str, limit: u32) -> Result<Vec<PathBuf>, String> {
        self.client()?.complete(typed, limit)
    }

    /// Opens `path`: a file in its default app, a folder in Nova Files or,
    /// in the launcher mode, the user's file manager.
    pub fn open(&self, path: &Path) -> Result<(), String> {
        let client = self.client()?;
        if !self.own_windows && path.is_dir() { client.show_folder_elsewhere(path) } else { client.open(path) }
    }

    /// Shows `path` in the folder that holds it: a Nova Files window or, in
    /// the launcher mode, the user's file manager.
    pub fn show_in_folder(&self, path: &Path) -> Result<(), String> {
        let client = self.client()?;
        if self.own_windows { client.show_in_folder(path) } else { client.show_item_elsewhere(path) }
    }

    /// Connects to Nova Files on `sources`' bus, in a mode with its own
    /// windows when `own_windows` (see [`windows_in`]). One it can't reach
    /// is logged, and every search then says so.
    pub fn start(sources: &Sources, own_windows: bool, logger: Arc<Logger>) -> Self {
        let client = Client::connect(&sources.bus, CALL_TIMEOUT);
        if let Err(e) = &client {
            logger.warn(LOG_SOURCE, &format!("can't reach Nova Files, so the launcher finds no files: {e}"));
        }
        let mime = Mime::follow(sources.data_dirs.clone(), logger);
        Self { client, mime, home: sources.home.clone(), own_windows }
    }

    /// Whether folders open in its own windows: what it started with.
    pub fn own_windows(&self) -> bool {
        self.own_windows
    }

    /// Stops reading signals and following the MIME database.
    pub fn stop(&self) {
        if let Ok(client) = &self.client {
            client.stop();
        }
        self.mime.stop();
    }

    fn client(&self) -> Result<&Client, String> {
        self.client.as_ref().map_err(|e| format!("Nova Files isn't reachable: {e}"))
    }
}

/// Whether Nova Files opens its own windows in `mode`, the feature's mode:
/// every mode but [`LAUNCHER_MODE`].
pub fn windows_in(mode: Option<&str>) -> bool {
    mode != Some(LAUNCHER_MODE)
}

#[cfg(test)]
pub mod stand_in;
#[cfg(test)]
mod tests;
