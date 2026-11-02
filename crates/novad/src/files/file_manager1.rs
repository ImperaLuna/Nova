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
            self.shared.open(window).map_err(zbus::fdo::Error::LimitsExceeded)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::{Files, Sources};
    use crate::hub::{ClientQueue, Hub, Outgoing, next_named};
    use crate::logger::Logger;
    use crate::session_bus::{Bus, TestBus};
    use nova_protocol::LogLevel;
    use nova_test_support::{TempDir, wait_until};
    use serde_json::json;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc::Receiver;

    const WAIT: Duration = Duration::from_secs(5);

    /// A private bus, Files serving on it, and a folder of things to show.
    struct Fixture {
        dir: TempDir,
        bus: TestBus,
        hub: Arc<Hub>,
        files: Arc<Files>,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir = TempDir::new(&format!("file-manager1-{name}"));
            std::fs::create_dir_all(dir.join("Downloads")).unwrap();
            std::fs::write(dir.join("Downloads/report one.pdf"), b"").unwrap();
            let bus = TestBus::start(&dir.join("bus"));
            let hub = Arc::new(Hub::default());
            let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
            let sources = Sources { bus: Bus::Address(bus.address.clone()), ui: None };
            let files = Files::start(sources, Arc::clone(&hub), logger);
            Self { dir, bus, hub, files }
        }

        /// The Nova Files process's connection, attached.
        fn nova_files(&self) -> (Receiver<Outgoing>, UnixStream) {
            let (stream, ours) = UnixStream::pair().unwrap();
            let (queue, outgoing) = ClientQueue::new(&ours).unwrap();
            self.files.attach(self.hub.register(queue), true);
            (outgoing, stream)
        }

        /// An app calling `method` with `uris`, as Firefox's "Show in folder" does.
        fn call(&self, method: &str, uris: &[String]) -> zbus::Result<zbus::Message> {
            let app = self.bus.connect();
            zbus::block_on(app.call_method(Some(BUS_NAME), PATH, Some(BUS_NAME), method, &(uris, "")))
        }

        fn path(&self, relative: &str) -> String {
            self.dir.join(relative).display().to_string()
        }
    }

    #[test]
    fn show_items_opens_the_folder_with_the_item_selected() {
        let fixture = Fixture::new("items");
        let (nova_files, _stream) = fixture.nova_files();
        let report = format!("file://{}/report%20one.pdf", fixture.path("Downloads"));
        fixture.call("ShowItems", &[report]).unwrap();
        let window = next_named(&nova_files, "files_window", WAIT);
        let expected =
            json!({"folder": fixture.path("Downloads"), "select": [fixture.path("Downloads/report one.pdf")]});
        assert_eq!(window, expected);
    }

    #[test]
    fn show_folders_opens_each_folder() {
        let fixture = Fixture::new("folders");
        let (nova_files, _stream) = fixture.nova_files();
        fixture.call("ShowFolders", &[format!("file://{}", fixture.path("Downloads"))]).unwrap();
        assert_eq!(next_named(&nova_files, "files_window", WAIT), json!({"folder": fixture.path("Downloads")}));
    }

    #[test]
    fn what_isnt_a_local_folder_is_refused() {
        let fixture = Fixture::new("refused");
        let error = fixture.call("ShowFolders", &["https://example.com/".into()]).unwrap_err();
        assert!(error.to_string().contains("isn't a local file"), "{error}");
        let missing = format!("file://{}", fixture.path("Missing"));
        assert!(fixture.call("ShowFolders", &[missing]).is_err());
        assert!(
            fixture.call("ShowItems", &vec![String::new(); MAX_URIS + 1]).unwrap_err().to_string().contains("URIs")
        );
    }

    #[test]
    fn stopping_gives_the_name_up() {
        let fixture = Fixture::new("stop");
        let app = fixture.bus.connect();
        let owned = || {
            let bus = "org.freedesktop.DBus";
            let reply = zbus::block_on(app.call_method(
                Some(bus),
                "/org/freedesktop/DBus",
                Some(bus),
                "NameHasOwner",
                &(BUS_NAME,),
            ));
            reply.unwrap().body().deserialize::<bool>().unwrap()
        };
        assert!(owned());
        fixture.files.stop();
        wait_until(WAIT, "FileManager1 given up", || !owned());
    }
}
