//! A stand-in for Nova Files' `Search1` on a test bus, for the tests of
//! the client and of the launcher's Files mode through it. It answers the
//! way Nova Files does: stages as signals sent to the caller only, and it
//! notes what it was asked to open or show.

use super::NovaFiles;
use super::client::{APP_ID, Client, PATH};
use crate::lock;
use crate::logger::Logger;
use crate::mime::Mime;
use nova_protocol::LogLevel;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

const TIMEOUT: Duration = Duration::from_secs(5);

/// What the stand-in was asked.
#[derive(Default)]
pub struct Asked {
    pub searches: Vec<(u64, String, u32)>,
    pub cancelled: Vec<u64>,
    pub opened: Vec<String>,
    pub shown: Vec<String>,
}

/// The stand-in, serving while it lives.
pub struct StandIn {
    pub asked: Arc<Mutex<Asked>>,
    _connection: zbus::Connection,
}

impl StandIn {
    /// Serves as Nova Files on the bus at `address`; what it finds is under
    /// `root`.
    pub fn serve(address: &str, root: &str) -> Self {
        let asked = Arc::default();
        let search = Search1 { asked: Arc::clone(&asked), root: root.to_string() };
        let builder = zbus::connection::Builder::address(address).name(APP_ID).serve_at(PATH, search);
        Self { asked, _connection: zbus::block_on(builder.build()).unwrap() }
    }
}

/// A client of the bus at `address`.
pub fn client(address: &str) -> Client {
    Client::connect(&crate::session_bus::Bus::Address(address.to_string()), TIMEOUT).unwrap()
}

/// The feature's module on the bus at `address`, with Home at `home`; it
/// keeps its log under `dir`.
pub fn nova_files(address: &str, dir: &Path, home: Option<PathBuf>) -> NovaFiles {
    NovaFiles { client: Ok(client(address)), mime: mime(dir), home }
}

/// An empty MIME database, logging under `dir`.
pub fn mime(dir: &Path) -> Arc<Mime> {
    let logger = Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::default()).unwrap();
    Mime::follow(Vec::new(), Arc::new(logger))
}

struct Search1 {
    asked: Arc<Mutex<Asked>>,
    root: String,
}

#[zbus::interface(name = "io.github.ImperaLuna.NovaFiles.Search1")]
impl Search1 {
    /// `kit` finds two stages, `slow` one that's never done, `broken`
    /// fails, and anything else finds nothing.
    async fn search(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        id: u64,
        query: String,
        limit: u32,
    ) {
        lock(&self.asked).searches.push((id, query.clone(), limit));
        let caller = header.sender().unwrap().to_owned();
        let to_caller = emitter.set_destination(caller.into());
        let hit = |name: &str| (format!("{}/{name}", self.root), "contains".to_string());
        match query.as_str() {
            "kit" => {
                Self::found(&to_caller, id, vec![hit("kitchen.txt")], false).await.unwrap();
                Self::found(&to_caller, id, vec![hit("kitcen notes.md")], true).await.unwrap();
            }
            "slow" => Self::found(&to_caller, id, vec![hit("slow.txt")], false).await.unwrap(),
            "broken" => Self::failed(&to_caller, id, "index-failed".into()).await.unwrap(),
            _ => Self::found(&to_caller, id, vec![], true).await.unwrap(),
        }
    }

    fn cancel(&self, id: u64) {
        lock(&self.asked).cancelled.push(id);
    }

    fn open(&self, path: String, _platform_data: HashMap<String, zbus::OwnedVariant>) {
        lock(&self.asked).opened.push(path);
    }

    fn show_in_folder(&self, path: String, _platform_data: HashMap<String, zbus::OwnedVariant>) {
        lock(&self.asked).shown.push(path);
    }

    #[zbus(signal)]
    async fn found(emitter: &SignalEmitter<'_>, id: u64, hits: Vec<(String, String)>, done: bool) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn failed(emitter: &SignalEmitter<'_>, id: u64, reason: String) -> zbus::Result<()>;
}
