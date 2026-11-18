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
}

impl NovaFiles {
    /// The paths Nova Files finds for `query`, best first, at most `limit`
    /// per stage.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<PathBuf>, String> {
        self.client()?.search(query, limit, STAGES_WAIT)
    }

    /// Opens `path`: a file in its default app, a folder in Nova Files.
    pub fn open(&self, path: &Path) -> Result<(), String> {
        self.client()?.open(path)
    }

    /// Opens a Nova Files window on the folder that holds `path`.
    pub fn show_in_folder(&self, path: &Path) -> Result<(), String> {
        self.client()?.show_in_folder(path)
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

    fn start(ctx: &crate::modules::Context, sources: &Sources, _: &(), _: &[FeatureState]) -> Arc<Self> {
        let client = Client::connect(&sources.bus, CALL_TIMEOUT);
        if let Err(e) = &client {
            ctx.logger.warn(LOG_SOURCE, &format!("can't reach Nova Files, so the launcher finds no files: {e}"));
        }
        let mime = Mime::follow(sources.data_dirs.clone(), Arc::clone(&ctx.logger));
        Arc::new(Self { client, mime, home: sources.home.clone() })
    }

    fn stop(&self) {
        if let Ok(client) = &self.client {
            client.stop();
        }
        self.mime.stop();
    }
}

#[cfg(test)]
pub mod stand_in;
#[cfg(test)]
mod tests;
