//! The standalone Nova Files app (feature `nova-files`): it answers the
//! launcher's Files mode (`launcher.rs`), finding files by name and opening
//! them or showing them in their folder.
//!
//! Nova Files serves `io.github.ImperaLuna.NovaFiles.Search1` at its app id,
//! which the bus starts it for (D-Bus activation) and which it leaves after
//! a few idle minutes. `Search` returns at once and its stages come as
//! `Found` signals sent only to the caller, until one is `done`; a search
//! waits [`STAGES_WAIT`] for that, then answers with the stages it has.
//! Starting a search cancels the one before if it's still going. `Open`
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
//! MIME database (`crate::mime`) types what's found, for its icon.

mod client;
mod launcher;

use crate::mime::Mime;
use crate::session_bus::Bus;
use client::Client;
use nova_protocol::FeatureState;
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

    fn client(&self) -> Result<&Client, String> {
        self.client.as_ref().map_err(|e| format!("Nova Files isn't reachable: {e}"))
    }
}

impl crate::modules::Module for NovaFiles {
    const ID: &'static str = ID;
    type Sources = Sources;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> Sources {
        let home = env.home.clone().filter(|home| home != Path::new("/"));
        Sources { bus: Bus::Session, data_dirs: crate::data_dirs(), home }
    }

    fn config(_: &crate::config::Config) {}

    fn start(ctx: &crate::modules::Context, sources: &Sources, _: &(), features: &[FeatureState]) -> Arc<Self> {
        let client = Client::connect(&sources.bus, CALL_TIMEOUT);
        if let Err(e) = &client {
            ctx.logger.warn(LOG_SOURCE, &format!("can't reach Nova Files, so the launcher finds no files: {e}"));
        }
        let mime = Mime::follow(sources.data_dirs.clone(), Arc::clone(&ctx.logger));
        Arc::new(Self { client, mime, home: sources.home.clone(), own_windows: own_windows(features) })
    }

    fn stop(&self) {
        if let Ok(client) = &self.client {
            client.stop();
        }
        self.mime.stop();
    }
}

/// Whether Nova Files opens its own windows in the mode `features` give
/// it.
fn own_windows(features: &[FeatureState]) -> bool {
    let mode = features.iter().find(|feature| feature.id == ID).and_then(|feature| feature.mode.as_deref());
    mode != Some(LAUNCHER_MODE)
}

#[cfg(test)]
pub mod stand_in;
#[cfg(test)]
mod tests;
