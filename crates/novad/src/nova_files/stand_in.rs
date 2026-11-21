//! A stand-in for Nova Files' `Search1` on a test bus, for the tests of
//! the client and of the launcher's Files mode through it. It answers the
//! way Nova Files does: stages as signals sent to the caller only, and it
//! notes what it was asked to open or show. It serves the user's file
//! manager (`org.freedesktop.FileManager1`) on the same bus too, noting the
//! URIs it was asked to show.

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
    pub completions: Vec<(String, u32)>,
    pub cancelled: Vec<u64>,
    pub opened: Vec<String>,
    pub shown: Vec<String>,
    /// The file manager's `ShowFolders` and `ShowItems` URIs.
    pub folders_elsewhere: Vec<String>,
    pub items_elsewhere: Vec<String>,
}

/// The stand-in, serving while it lives.
pub struct StandIn {
    pub asked: Arc<Mutex<Asked>>,
    _connection: zbus::Connection,
    _file_manager: zbus::Connection,
}

impl StandIn {
    /// Serves as Nova Files on the bus at `address`; what it finds is under
    /// `root`.
    pub fn serve(address: &str, root: &str) -> Self {
        let asked = Arc::default();
        let search = Search1 { asked: Arc::clone(&asked), root: root.to_string() };
        let builder = zbus::connection::Builder::address(address).name(APP_ID).serve_at(PATH, search);
        let file_manager = FileManager1 { asked: Arc::clone(&asked) };
        let file_manager = zbus::connection::Builder::address(address)
            .name("org.freedesktop.FileManager1")
            .serve_at("/org/freedesktop/FileManager1", file_manager);
        let (connection, file_manager) = (builder.build(), file_manager.build());
        Self {
            asked,
            _connection: zbus::block_on(connection).unwrap(),
            _file_manager: zbus::block_on(file_manager).unwrap(),
        }
    }
}

/// A client of the bus at `address`.
pub fn client(address: &str) -> Client {
    Client::connect(&crate::session_bus::Bus::Address(address.to_string()), TIMEOUT).unwrap()
}

/// The feature's module on the bus at `address`, with Home at `home`, in a
/// mode with Nova Files' own windows unless `launcher_mode`; it keeps its
/// log under `dir`.
pub fn nova_files(address: &str, dir: &Path, home: Option<PathBuf>, launcher_mode: bool) -> NovaFiles {
    NovaFiles { client: Ok(client(address)), mime: mime(dir), home, own_windows: !launcher_mode }
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

    /// A typed path ending in `kit`, case aside, completes to the folder `kitchen.txt`
    /// and the file `kitcen notes.md`, `~nobody/…` isn't a path it takes,
    /// and anything else completes to nothing.
    fn complete(&self, typed: String, limit: u32) -> zbus::fdo::Result<Vec<(String, bool)>> {
        lock(&self.asked).completions.push((typed.clone(), limit));
        if typed.starts_with("~nobody") {
            return Err(zbus::fdo::Error::InvalidArgs(format!("not a path from / or ~: {typed}")));
        }
        let entry = |name: &str, folder: bool| (format!("{}/{name}", self.root), folder);
        Ok(if typed.to_lowercase().ends_with("kit") {
            vec![entry("kitchen.txt", true), entry("kitcen notes.md", false)]
        } else {
            vec![]
        })
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

struct FileManager1 {
    asked: Arc<Mutex<Asked>>,
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl FileManager1 {
    fn show_folders(&self, uris: Vec<String>, _startup_id: String) {
        lock(&self.asked).folders_elsewhere.extend(uris);
    }

    fn show_items(&self, uris: Vec<String>, _startup_id: String) {
        lock(&self.asked).items_elsewhere.extend(uris);
    }
}
