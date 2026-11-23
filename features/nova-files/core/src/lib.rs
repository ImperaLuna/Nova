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
pub const LAUNCHER_MODE: &str = "launcher";
const LOG_SOURCE: &str = "nova-files";
pub const STAGES_WAIT: Duration = Duration::from_millis(100);
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Sources {
    pub bus: Bus,
    pub data_dirs: Vec<PathBuf>,
    pub home: Option<PathBuf>,
}

pub struct NovaFiles {
    client: Result<Client, String>,
    mime: Arc<Mime>,
    home: Option<PathBuf>,
    own_windows: bool,
}

impl NovaFiles {
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<PathBuf>, String> {
        self.client()?.search(query, limit, STAGES_WAIT)
    }

    pub fn complete(&self, typed: &str, limit: u32) -> Result<Vec<PathBuf>, String> {
        self.client()?.complete(typed, limit)
    }

    pub fn open(&self, path: &Path) -> Result<(), String> {
        let client = self.client()?;
        if !self.own_windows && path.is_dir() { client.show_folder_elsewhere(path) } else { client.open(path) }
    }

    pub fn show_in_folder(&self, path: &Path) -> Result<(), String> {
        let client = self.client()?;
        if self.own_windows { client.show_in_folder(path) } else { client.show_item_elsewhere(path) }
    }

    pub fn start(sources: &Sources, own_windows: bool, logger: Arc<Logger>) -> Self {
        let client = Client::connect(&sources.bus, CALL_TIMEOUT);
        if let Err(e) = &client {
            logger.warn(LOG_SOURCE, &format!("can't reach Nova Files, so the launcher finds no files: {e}"));
        }
        let mime = Mime::follow(sources.data_dirs.clone(), logger);
        Self { client, mime, home: sources.home.clone(), own_windows }
    }

    pub fn own_windows(&self) -> bool {
        self.own_windows
    }

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

pub fn windows_in(mode: Option<&str>) -> bool {
    mode != Some(LAUNCHER_MODE)
}

#[cfg(test)]
pub mod stand_in;
#[cfg(test)]
mod tests;
