//! Nova Files (feature `files`): the file manager's daemon side. The
//! windows are their own Quickshell process, started on demand, and only
//! render; what they show and do is worked out here.
//!
//! One file per job, all under this folder:
//! - `mod.rs`: the module, and the windows asked for (`nova files`,
//!   [`Files::show`]) that the Nova Files process hasn't been sent yet.
//! - `target.rs`: what a window shows, checked: an absolute folder that
//!   exists, and items in it to select.
//! - `file_manager1.rs`: `org.freedesktop.FileManager1`, so apps' "Show in
//!   folder" opens Nova Files.
//! - `window.rs`: the Nova Files process (`ui/files.qml`), started when a
//!   window is asked for and none runs.
//! - Opening files in apps is the shared `crate::apps::opening`, over the
//!   module's one copy of the MIME database.
//! - `views.rs`: the folders a window shows (`files_open`), read in pages
//!   and followed while open, one thread per view.
//! - `watcher.rs`: the one inotify instance every view follows its folder
//!   through, and the folders above it, so a renamed parent is seen.
//! - `trash.rs`: the trash, shared with other apps (freedesktop Trash
//!   spec): moving to it, restoring, deleting for good, and following it
//!   so windows hear when it changes.
//! - `folder.rs`: a folder's entries, their types from the shared MIME
//!   database (`crate::mime`, read as the module starts and again when a
//!   package brings new types), and the view's order.
//! - `tree.rs`: the sidebar's folder tree that follows the window
//!   (`files_tree`) and its places (`files_places`).
//! - `search/`: finding files by name (`files_search`) from an index of
//!   every name under Home and the drives, kept up to date with inotify.
//! - `launcher.rs`: the launcher's Files mode, a launcher provider that
//!   searches that index or completes a typed path.
//! - `udisks.rs`: the drives in the sidebar, from udisks2, and mounting
//!   and ejecting them; `drives.rs`: which filesystems are listed and
//!   what they're called.
//! - `transfers.rs`: copy and move jobs (`files_transfer`), one thread
//!   each, asking the window about name clashes.
//! - `copying.rs`: the file system side of those: measuring, numbered
//!   names, and copying a file in chunks.
//!
//! How solid the windows are (`[files]` in config.toml) is applied in place
//! and published on [`Topic::Files`] as [`Event::FilesLook`], so the open
//! windows follow an edit at once.
//!
//! Windows go only to the Nova Files process, once it has attached
//! (`attach_files`, [`Files::attach`]); no other client on [`Topic::Files`]
//! takes them. One asked for while none is attached waits, and starts one,
//! and goes to it when it attaches. Asking, attaching and detaching take
//! the same lock, so none is lost or sent twice. A process that detached as
//! it quit, with windows asked for since, is followed by a new one.

mod copying;
mod drives;
mod file_manager1;
mod folder;
mod launcher;
pub mod search;
mod target;
mod transfers;
mod trash;
pub mod tree;
mod udisks;
mod views;
mod watcher;
mod window;

use crate::apps::AppIndex;
use crate::apps::associations::Places;
use crate::apps::launch::Runner;
use crate::apps::opening::Opener;
use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use crate::notifications::Notifications;
use crate::session_bus::{Bus, Stop};
use nova_protocol::{
    ClashAnswer, EjectOutcome, Event, FeatureState, FilePlace, FileSort, FilesLook, FilesWindow, FolderTree, Topic,
    TransferMode,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

pub use trash::Trash;

pub const FEATURE_ID: &str = "files";
const LOG_SOURCE: &str = "files";
/// More windows than anyone asks for before Nova Files starts; past it,
/// more are refused rather than kept.
const MAX_WAITING: usize = 64;
/// The home trash, in the data folder.
const TRASH_DIR: &str = "Trash";
/// This process's mount table.
const MOUNT_TABLE: &str = "/proc/self/mountinfo";

/// Where Nova Files runs from.
#[derive(Clone)]
pub struct Sources {
    /// Where `org.freedesktop.FileManager1` is served.
    pub bus: Bus,
    /// Where udisks2 is: the system bus.
    pub udisks: Bus,
    /// The data folders the MIME database is read from.
    pub data_dirs: Vec<PathBuf>,
    /// Where the `mimeapps.list` files are.
    pub associations: Places,
    /// How apps opening files are started.
    pub runner: Runner,
    /// The user's Home, where the sidebar's tree starts.
    pub home: PathBuf,
    /// How the Nova Files process runs; none for a novad without its UI,
    /// where windows only wait.
    pub ui: Option<crate::supervisor::UiSettings>,
    /// What the search index covers.
    pub search: search::Sources,
    /// The home trash (`$XDG_DATA_HOME/Trash`); none without a home.
    pub trash: Option<PathBuf>,
    /// The mount table, where drives' trashes are looked for.
    pub mounts: PathBuf,
}

pub struct Files {
    shared: Arc<Shared>,
    /// Opens files in apps.
    opener: Arc<Opener>,
    /// The MIME database, followed while the module runs.
    mime: Arc<crate::mime::Mime>,
    /// Ends the FileManager1 service.
    stop: Arc<Stop>,
    /// The folders the windows show.
    folders: views::Folders,
    /// The index of file names.
    search: search::Search,
    /// How solid the windows are, as last published.
    look: Mutex<FilesLook>,
    /// The copies and moves they run.
    transfers: transfers::Transfers,
    trash: Arc<Trash>,
    /// Tells windows when the trash changes; none when it can't be followed.
    trash_watch: Option<trash::Watch>,
    /// The drives in the sidebar.
    drives: udisks::Drives,
}

/// What the module's services share.
struct Shared {
    sources: Sources,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    delivery: Mutex<Delivery>,
    process: Option<window::Process>,
}

/// Where windows go, under one lock.
#[derive(Default)]
struct Delivery {
    /// The Nova Files process's connection, while it's attached.
    attached: Option<ClientId>,
    /// Windows asked for that no Nova Files process has been sent yet.
    waiting: Vec<FilesWindow>,
    /// The process detached to quit; once it exits, windows still waiting
    /// start the next one.
    quitting: bool,
}

impl Files {
    /// Starts it, opening files in `apps` with programs from `path_var`.
    pub fn start(
        sources: Sources,
        hub: Arc<Hub>,
        logger: Arc<Logger>,
        apps: Arc<AppIndex>,
        path_var: &str,
    ) -> Arc<Self> {
        let mime = crate::mime::Mime::follow(sources.data_dirs.clone(), Arc::clone(&logger));
        let folders = views::Folders::new(Arc::clone(&hub), Arc::clone(&logger), Arc::clone(&mime));
        let search = search::Search::start(sources.search.clone(), Arc::clone(&logger), Arc::clone(&mime));
        let transfers = transfers::Transfers::new(Arc::clone(&hub), Arc::clone(&logger));
        let trash = Arc::new(Trash::new(sources.trash.clone(), sources.mounts.clone(), Arc::clone(&mime)));
        let trash_watch = trash::Watch::start(Arc::clone(&trash), Arc::clone(&hub), Arc::clone(&logger));
        let opener = Opener::new(
            Arc::clone(&mime),
            sources.associations.clone(),
            sources.runner.clone(),
            apps,
            path_var,
            Arc::clone(&logger),
        );
        let shared = Arc::new_cyclic(|shared: &Weak<Shared>| {
            let shared = Weak::clone(shared);
            let exited = move || {
                if let Some(shared) = shared.upgrade() {
                    shared.exited();
                }
            };
            let process = sources.ui.clone().map(|ui| window::Process::new(ui, Arc::clone(&logger), exited));
            Shared { sources, hub, logger, delivery: Mutex::default(), process }
        });
        let drives = udisks::Drives::start(
            shared.sources.udisks.clone(),
            // Mounts in the home folder are listed too; "/" is no home.
            Some(shared.sources.home.clone()).filter(|home| home.as_path() != Path::new("/")),
            Arc::clone(&shared.hub),
            Arc::clone(&shared.logger),
        );
        let stop = Arc::new(Stop::default());
        file_manager1::start(&shared, &stop);
        Arc::new(Self {
            shared,
            opener: Arc::new(opener),
            mime,
            stop,
            folders,
            look: Mutex::default(),
            transfers,
            trash,
            trash_watch,
            drives,
            search,
        })
    }

    /// What opens files in apps.
    pub fn opener(&self) -> Arc<Opener> {
        Arc::clone(&self.opener)
    }

    /// The trash.
    pub fn trash(&self) -> Arc<Trash> {
        Arc::clone(&self.trash)
    }

    /// What finds files by name (`files_search`).
    pub fn searcher(&self) -> search::Searcher {
        self.search.searcher()
    }

    /// Shows the folder `path` in client `client`'s view `view`
    /// (`files_open`, [`views::Folders::open`]).
    /// How full the drives are is read again too, since a window going
    /// somewhere is when it's looked at.
    pub fn open(&self, client: ClientId, view: u32, path: &str, sort: FileSort, hidden: bool) -> Result<(), String> {
        self.folders.open(client, view, path, sort, hidden)?;
        self.drives.refresh();
        Ok(())
    }

    /// Mounts drive `drive` (`files_mount`) and answers where. Waits on
    /// udisks2, so it runs off the connection's thread.
    pub fn mount(&self, drive: &str) -> Result<String, String> {
        self.drives.mount(drive)
    }

    /// Ejects drive `drive` (`files_eject`). Waits on udisks2 writing what's
    /// left, so it runs off the connection's thread.
    pub fn eject(&self, drive: &str) -> Result<EjectOutcome, String> {
        self.drives.eject(drive)
    }

    /// The sidebar's tree for a window on `request.folder` (`files_tree`,
    /// [`tree::tree`]). It reads folders, so it's called off the
    /// connection's thread.
    pub fn tree(&self, request: &tree::Request) -> Result<FolderTree, String> {
        tree::tree(request, &self.shared.sources.home)
    }

    /// The sidebar's places that exist (`files_places`).
    pub fn places(&self) -> Vec<FilePlace> {
        tree::places(&self.shared.sources.home, nova_paths::user_dir)
    }

    /// Closes client `client`'s view `view` (`files_close`).
    pub fn close(&self, client: ClientId, view: u32) {
        self.folders.close(client, view);
    }

    /// Starts client `client`'s job `job`, copying or moving `sources` into
    /// `folder` (`files_transfer`, [`transfers::Transfers::start`]).
    pub fn transfer(
        &self,
        client: ClientId,
        job: u32,
        mode: TransferMode,
        sources: &[String],
        folder: &str,
    ) -> Result<(), String> {
        self.transfers.start(client, job, mode, sources, folder)
    }

    /// Stops client `client`'s job `job` (`files_transfer_cancel`).
    pub fn cancel_transfer(&self, client: ClientId, job: u32) {
        self.transfers.cancel(client, job);
    }

    /// Answers a name clash of client `client`'s job `job`
    /// (`files_transfer_answer`).
    pub fn answer_clash(
        &self,
        client: ClientId,
        job: u32,
        clash: u32,
        answer: ClashAnswer,
        all: bool,
    ) -> Result<(), String> {
        self.transfers.answer(client, job, clash, answer, all)
    }

    /// Opens a window on `folder` with `select` selected, once both check
    /// out ([`target::window`]).
    pub fn show(&self, folder: &str, select: &[String]) -> Result<(), String> {
        self.shared.open(target::window(folder, select)?)
    }

    /// Client `client`, the Nova Files process, can open windows
    /// (`attached`), and is sent those waiting; or it's quitting, and gets
    /// no more.
    pub fn attach(&self, client: ClientId, attached: bool) {
        let mut delivery = lock(&self.shared.delivery);
        if attached {
            delivery.attached = Some(client);
            delivery.quitting = false;
            let waiting = std::mem::take(&mut delivery.waiting);
            let mut unsent = waiting.into_iter().skip_while(|window| self.shared.send(client, window));
            if let Some(window) = unsent.next() {
                // It hung up already.
                delivery.attached = None;
                delivery.waiting = std::iter::once(window).chain(unsent).collect();
            }
        } else if delivery.attached == Some(client) {
            delivery.attached = None;
            delivery.quitting = true;
        }
    }

    /// Makes the windows as solid as `look` says, telling them when that's
    /// a change. Starts from all glass, which the windows assume too.
    pub fn set_look(&self, look: FilesLook) {
        let mut current = lock(&self.look);
        if *current != look {
            *current = look;
            self.shared.hub.publish(&Event::FilesLook(look));
        }
    }

    /// Sends client `client`, which just subscribed, how solid the windows are.
    pub fn send_state(&self, client: ClientId) {
        let look = *lock(&self.look);
        self.shared.hub.send_to(client, &Event::FilesLook(look));
        self.drives.send_state(client);
    }

    /// Client `client` hung up: windows aren't sent to it any more.
    pub fn client_gone(&self, client: ClientId) {
        self.folders.client_gone(client);
        self.transfers.client_gone(client);
        let mut delivery = lock(&self.shared.delivery);
        if delivery.attached == Some(client) {
            delivery.attached = None;
        }
    }

    /// Stops it: it publishes nothing more, gives up FileManager1, closes
    /// Nova Files' windows and folder views, stops copies, following the
    /// MIME database and keeping the search index, and drops windows still
    /// waiting.
    pub fn stop(&self) {
        self.stop.stop();
        self.mime.stop();
        self.folders.stop();
        self.search.stop();
        self.drives.stop();
        self.transfers.stop();
        if let Some(watch) = &self.trash_watch {
            watch.stop();
        }
        if let Some(process) = &self.shared.process {
            process.stop();
        }
        *lock(&self.shared.delivery) = Delivery::default();
    }
}

impl Shared {
    /// Sends `window` to the Nova Files process, or keeps it for the next
    /// one to attach, starting one, unless [`MAX_WAITING`] already wait.
    fn open(&self, window: FilesWindow) -> Result<(), String> {
        let mut delivery = lock(&self.delivery);
        if let Some(client) = delivery.attached {
            if self.send(client, &window) {
                return Ok(());
            }
            delivery.attached = None;
        }
        if delivery.waiting.len() >= MAX_WAITING {
            return Err(format!("{MAX_WAITING} windows already wait for Nova Files to start"));
        }
        self.logger.info(LOG_SOURCE, &format!("{} waits for Nova Files to start", window.folder));
        delivery.waiting.push(window);
        if let Some(process) = &self.process {
            process.ensure_running();
        }
        Ok(())
    }

    fn send(&self, client: ClientId, window: &FilesWindow) -> bool {
        self.hub.send_to(client, &Event::FilesWindow(window.clone()))
    }

    /// The Nova Files process exited. One that quit while windows were asked
    /// for is followed by a new one; one that crashed isn't, until the next
    /// window is asked for.
    fn exited(&self) {
        let mut delivery = lock(&self.delivery);
        if std::mem::take(&mut delivery.quitting)
            && !delivery.waiting.is_empty()
            && let Some(process) = &self.process
        {
            process.ensure_running();
        }
    }
}

/// Its options are how solid the windows are, applied in place.
impl crate::modules::Module for Files {
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<Topic> = Some(Topic::Files);
    /// Files open in apps.
    const NEEDS_APPS: bool = true;
    type Sources = Sources;
    type Config = FilesLook;

    fn sources(env: &crate::modules::Env) -> Sources {
        let data_dirs = crate::data_dirs();
        let associations = Places::system(&data_dirs);
        let runner = Runner::detect(env.path_var);
        let home = nova_paths::home().unwrap_or_else(|| PathBuf::from("/"));
        let trash = nova_paths::data_home().map(|data| data.join(TRASH_DIR));
        let search = search::Sources::system(env.home.clone());
        Sources {
            bus: Bus::Session,
            udisks: Bus::System,
            data_dirs,
            associations,
            runner,
            home,
            ui: env.ui.cloned(),
            trash,
            mounts: MOUNT_TABLE.into(),
            search,
        }
    }

    fn config(config: &crate::config::Config) -> FilesLook {
        config.files.look
    }

    fn start(ctx: &crate::modules::Context, sources: &Sources, look: &FilesLook, _: &[FeatureState]) -> Arc<Self> {
        let files = Self::start(
            sources.clone(),
            Arc::clone(&ctx.hub),
            Arc::clone(&ctx.logger),
            Arc::clone(&ctx.apps),
            &ctx.path_var,
        );
        files.set_look(*look);
        files
    }

    fn reconfigure(&self, _: &FilesLook, look: &FilesLook, _: &[FeatureState]) -> crate::modules::Reconfigure {
        self.set_look(*look);
        crate::modules::Reconfigure::InPlace
    }

    fn send_state(&self, client: ClientId) {
        Self::send_state(self, client);
    }

    fn client_gone(&self, client: ClientId) {
        Self::client_gone(self, client);
    }

    /// "Safe To Remove Hardware" goes to the notifications, while they're on.
    fn linked(&self, running: &crate::modules::Running) {
        self.drives.set_notifications(running.get::<Notifications>().as_ref());
    }

    fn stop(&self) {
        Self::stop(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{ClientQueue, Outgoing, next_named};
    use nova_protocol::LogLevel;
    use nova_test_support::TempDir;
    use serde_json::json;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(5);

    /// Files with no UI to start, and a folder to show.
    struct Fixture {
        dir: TempDir,
        hub: Arc<Hub>,
        files: Arc<Files>,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir = TempDir::new(&format!("files-{name}"));
            std::fs::create_dir_all(dir.join("Downloads")).unwrap();
            // What the search index covers (`search::Sources::under`).
            std::fs::create_dir_all(dir.join("home")).unwrap();
            let hub = Arc::new(Hub::default());
            let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
            let no_bus = Bus::Address(format!("unix:path={}", dir.join("no-bus").display()));
            let sources = Sources {
                bus: no_bus.clone(),
                udisks: no_bus,
                data_dirs: vec![],
                associations: crate::apps::associations::Places::new(None, &[], &[], &[]),
                runner: crate::apps::launch::Runner::Direct,
                home: dir.to_path_buf(),
                ui: None,
                search: search::Sources::under(&dir),
                trash: Some(dir.join("Trash")),
                mounts: dir.join("no-mountinfo"),
            };
            let apps = crate::apps::AppIndex::of(None, &logger);
            let files = Files::start(sources, Arc::clone(&hub), logger, apps, "");
            Self { dir, hub, files }
        }

        /// A connection subscribed to the files topic, as `nova ipc
        /// subscribe files` is.
        fn client(&self) -> (ClientId, Receiver<Outgoing>, UnixStream) {
            let (stream, ours) = UnixStream::pair().unwrap();
            let (queue, outgoing) = ClientQueue::new(&ours).unwrap();
            let client = self.hub.register(queue);
            self.hub.subscribe(client, Topic::Files);
            (client, outgoing, stream)
        }

        /// The Nova Files process's connection, attached.
        fn nova_files(&self) -> (ClientId, Receiver<Outgoing>, UnixStream) {
            let connection = self.client();
            self.files.attach(connection.0, true);
            connection
        }

        fn downloads(&self) -> String {
            self.dir.join("Downloads").display().to_string()
        }
    }

    #[test]
    fn a_window_waits_until_nova_files_attaches() {
        let fixture = Fixture::new("waits");
        fixture.files.show(&fixture.downloads(), &[]).unwrap();
        // Another subscriber to the topic takes nothing.
        let (_, watcher, _watcher_stream) = fixture.client();
        let (_, nova_files, _stream) = fixture.nova_files();
        assert_eq!(next_named(&nova_files, "files_window", WAIT)["folder"], json!(fixture.downloads()));
        fixture.files.show(&fixture.downloads(), &[]).unwrap();
        assert_eq!(next_named(&nova_files, "files_window", WAIT)["folder"], json!(fixture.downloads()));
        assert!(watcher.try_recv().is_err(), "windows go only to Nova Files");
    }

    #[test]
    fn subscribers_hear_how_solid_the_windows_are() {
        let fixture = Fixture::new("look");
        let (client, events, _stream) = fixture.client();
        fixture.files.send_state(client);
        assert_eq!(next_named(&events, "files_look", WAIT), json!({"frame": 0, "list": 0}));
        fixture.files.set_look(FilesLook { frame: 0, list: 100 });
        assert_eq!(next_named(&events, "files_look", WAIT), json!({"frame": 0, "list": 100}));
        // The same look again tells nobody.
        fixture.files.set_look(FilesLook { frame: 0, list: 100 });
        fixture.files.send_state(client);
        assert_eq!(next_named(&events, "files_look", WAIT), json!({"frame": 0, "list": 100}));
        let more_looks =
            events.try_iter().any(|sent| matches!(sent, Outgoing::Line(line) if line.contains("files_look")));
        assert!(!more_looks, "one event per change, one per subscriber");
    }

    #[test]
    fn windows_past_the_limit_are_refused_while_none_attaches() {
        let fixture = Fixture::new("limit");
        for _ in 0..MAX_WAITING {
            fixture.files.show(&fixture.downloads(), &[]).unwrap();
        }
        assert!(fixture.files.show(&fixture.downloads(), &[]).unwrap_err().contains("already wait"));
    }

    #[test]
    fn a_window_asked_for_as_nova_files_quits_waits_for_the_next() {
        let fixture = Fixture::new("quits");
        let (client, quitting, _stream) = fixture.nova_files();
        fixture.files.attach(client, false);
        fixture.files.show(&fixture.downloads(), &[]).unwrap();
        let (_, next, _next_stream) = fixture.nova_files();
        assert_eq!(next_named(&next, "files_window", WAIT)["folder"], json!(fixture.downloads()));
        assert!(quitting.try_recv().is_err(), "nothing for the one quitting");
    }

    #[test]
    fn the_launchers_files_mode_finds_indexed_names_and_shows_them_in_nova_files() {
        use crate::launcher::providers::{Provider, Run};
        let fixture = Fixture::new("launcher");
        // The index covers `<dir>/home`; the fixture's Home is `<dir>`.
        std::fs::create_dir_all(fixture.dir.join("home/Games")).unwrap();
        std::fs::write(fixture.dir.join("home/Games/Crimson Desert.sh"), b"x").unwrap();
        let mut found = Vec::new();
        nova_test_support::wait_until(WAIT, "the index finds it", || {
            found = fixture.files.search("crim des").unwrap();
            !found.is_empty()
        });
        let game = &found[0];
        let path = fixture.dir.join("home/Games/Crimson Desert.sh").display().to_string();
        assert_eq!(game.id, path);
        assert_eq!(game.description.as_deref(), Some("~/home/Games"));
        assert!(game.name[0].matched && game.name[0].text == "Crim");
        assert_eq!(game.file.as_ref().unwrap().completion, "~/home/Games/Crimson Desert.sh");

        let (_, nova_files, _stream) = fixture.nova_files();
        let logger =
            Logger::open(&fixture.dir.join("run-logs"), LogLevel::Debug, false, Arc::clone(&fixture.hub)).unwrap();
        let how = |new_window| Run { output: None, new_window, compositor: None, tray: None, logger: &logger };
        // Shift+Enter: its folder, with it selected.
        fixture.files.run(&path, &how(true)).unwrap();
        let window = next_named(&nova_files, "files_window", WAIT);
        assert_eq!(window["folder"], json!(fixture.dir.join("home/Games").display().to_string()));
        assert_eq!(window["select"], json!([path]));
        // A folder opens in Nova Files.
        fixture.files.run(&fixture.downloads(), &how(false)).unwrap();
        assert_eq!(next_named(&nova_files, "files_window", WAIT)["folder"], json!(fixture.downloads()));
    }

    #[test]
    fn a_window_for_nova_files_that_hung_up_waits_for_the_next() {
        let fixture = Fixture::new("hung-up");
        let (client, _, stream) = fixture.nova_files();
        fixture.hub.unregister(client);
        drop(stream);
        fixture.files.show(&fixture.downloads(), &[]).unwrap();
        let (_, next, _next_stream) = fixture.nova_files();
        assert_eq!(next_named(&next, "files_window", WAIT)["folder"], json!(fixture.downloads()));
    }
}
