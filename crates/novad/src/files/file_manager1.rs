//! `org.freedesktop.FileManager1` on the session bus, so an app's "Show in
//! folder" (Firefox's downloads, VS Code, Telegram) opens Nova Files. The
//! name is queued behind a file manager that already owns it, and taken
//! over when that one quits; novad never replaces it.
//!
//! `ShowFolders` opens a window on each folder, `ShowItems` one on each
//! folder holding the items, with them selected. `ShowItemProperties` does
//! what `ShowItems` does until Nova Files has a properties view.

use super::{Shared, target};
use crate::session_bus::{self, Stop};
use std::future::poll_fn;
use std::sync::Arc;
use std::time::Duration;
use zbus::fdo::RequestNameReply;
use zbus::interface;

pub const BUS_NAME: &str = "org.freedesktop.FileManager1";
const PATH: &str = "/org/freedesktop/FileManager1";
/// How long novad waits for the bus to answer.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// More URIs than one call can mean; the call is refused.
const MAX_URIS: usize = 256;

/// Serves the interface on a thread of its own until `stop`. Returns once
/// the name is claimed, queued, or connecting failed (logged).
pub fn start(shared: &Arc<Shared>, stop: &Arc<Stop>) {
    let (ready, started) = std::sync::mpsc::channel();
    let serving = Arc::clone(shared);
    let stop = Arc::clone(stop);
    std::thread::spawn(move || {
        if let Err(e) = zbus::block_on(serve(&serving, &stop, &ready)) {
            serving.logger.warn(super::LOG_SOURCE, &format!("apps' \"Show in folder\" won't open Nova Files: {e}"));
        }
        let _ = ready.send(());
    });
    // Waits as long as connecting may take; a timed-out call fails sooner.
    let _ = started.recv_timeout(2 * CALL_TIMEOUT);
}

async fn serve(shared: &Arc<Shared>, stop: &Stop, ready: &std::sync::mpsc::Sender<()>) -> zbus::Result<()> {
    let server = Server { shared: Arc::clone(shared) };
    let connection = shared.sources.bus.builder().method_timeout(CALL_TIMEOUT).serve_at(PATH, server).build().await?;
    match session_bus::claim(&connection, BUS_NAME).await? {
        RequestNameReply::InQueue => shared.logger.warn(
            super::LOG_SOURCE,
            &format!("another file manager owns {BUS_NAME}; apps' \"Show in folder\" opens it until it quits"),
        ),
        _ => shared.logger.info(super::LOG_SOURCE, &format!("opening apps' \"Show in folder\" as {BUS_NAME}")),
    }
    let _ = ready.send(());
    poll_fn(|cx| stop.poll(cx)).await;
    // Dropping the connection gives the name up.
    drop(connection);
    Ok(())
}

/// The interface apps see on the bus.
struct Server {
    shared: Arc<Shared>,
}

impl Server {
    fn show(&self, windows: Result<Vec<nova_protocol::FilesWindow>, String>) -> zbus::fdo::Result<()> {
        let windows = windows.map_err(zbus::fdo::Error::InvalidArgs)?;
        for window in windows {
            self.shared.open(window);
        }
        Ok(())
    }
}

/// Refuses calls naming more URIs than [`MAX_URIS`].
fn capped(uris: &[String]) -> Result<&[String], String> {
    if uris.len() > MAX_URIS { Err(format!("over {MAX_URIS} URIs")) } else { Ok(uris) }
}

#[interface(name = "org.freedesktop.FileManager1")]
impl Server {
    fn show_folders(&self, uris: Vec<String>, _startup_id: String) -> zbus::fdo::Result<()> {
        self.show(capped(&uris).and_then(target::folders))
    }

    fn show_items(&self, uris: Vec<String>, _startup_id: String) -> zbus::fdo::Result<()> {
        self.show(capped(&uris).and_then(target::items))
    }

    fn show_item_properties(&self, uris: Vec<String>, _startup_id: String) -> zbus::fdo::Result<()> {
        self.show(capped(&uris).and_then(target::items))
    }
}
