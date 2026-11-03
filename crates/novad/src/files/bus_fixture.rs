//! A private session bus with Files serving on it and a folder of things
//! to show, for the tests of Files' bus services (FileManager1, the file
//! chooser portal).

use super::{Files, Sources};
use crate::hub::{ClientId, ClientQueue, Hub, Outgoing};
use crate::logger::Logger;
use crate::session_bus::{Bus, TestBus};
use nova_protocol::LogLevel;
use nova_test_support::TempDir;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

pub const WAIT: Duration = Duration::from_secs(5);

pub struct Fixture {
    pub dir: TempDir,
    pub bus: TestBus,
    pub hub: Arc<Hub>,
    pub files: Arc<Files>,
}

impl Fixture {
    /// Files on a new bus, with `Downloads/report one.pdf` in its home.
    pub fn new(name: &str) -> Self {
        let dir = TempDir::new(&format!("files-bus-{name}"));
        std::fs::create_dir_all(dir.join("Downloads")).unwrap();
        std::fs::write(dir.join("Downloads/report one.pdf"), b"").unwrap();
        let bus = TestBus::start(&dir.join("bus"));
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let sources = Sources {
            bus: Bus::Address(bus.address.clone()),
            udisks: Bus::Address(bus.address.clone()),
            data_dirs: vec![],
            associations: crate::apps::associations::Places::new(None, &[], &[], &[]),
            runner: crate::apps::launch::Runner::Direct,
            home: dir.to_path_buf(),
            ui: None,
            search: crate::files::search::Sources::under(&dir),
            trash: None,
            mounts: dir.join("no-mountinfo"),
            previews: None,
            opened: None,
            thumbnails: crate::files::thumbnails::Sources::under(&dir),
            bookmarks: None,
            user_folders: Vec::new(),
            recent: None,
            games: crate::files::games::Sources { steam: vec![] },
        };
        let apps = crate::apps::AppIndex::of(None, &logger);
        let files = Files::start(sources, Arc::clone(&hub), logger, apps, "");
        Self { dir, bus, hub, files }
    }

    /// The Nova Files process's connection, attached.
    pub fn nova_files(&self) -> (Receiver<Outgoing>, UnixStream) {
        let (_, outgoing, stream) = self.nova_files_client();
        (outgoing, stream)
    }

    /// The same, with its client id.
    pub fn nova_files_client(&self) -> (ClientId, Receiver<Outgoing>, UnixStream) {
        let (stream, ours) = UnixStream::pair().unwrap();
        let (queue, outgoing) = ClientQueue::new(&ours).unwrap();
        let client = self.hub.register(queue);
        self.files.attach(client, true);
        (client, outgoing, stream)
    }

    pub fn path(&self, relative: &str) -> String {
        self.dir.join(relative).display().to_string()
    }
}
