//! Nova's file chooser for xdg-desktop-portal: novad serves
//! `org.freedesktop.impl.portal.FileChooser` as
//! `org.freedesktop.impl.portal.desktop.nova` on the session bus, so an
//! app's Open or Save dialog (Firefox's uploads, VS Code's Open Folder) is
//! a Nova Files window in picker mode. The portal finds this backend from
//! `nova.portal` and is pointed at it by `portals.conf` (`nova setup`, the
//! NixOS module). Apps that don't use the portal keep their own dialog.
//!
//! A call becomes a [`FilePicker`] (its options checked and capped, its
//! filters turned into Nova's), waits in [`super::picker::Pickers`], and
//! opens a window like any other ([`super::Shared::open`]). The call then
//! waits, without holding the bus (zbus runs each call as its own task),
//! for the window's `files_pick` or `files_pick_cancel`. The portal's
//! `Request.Close` at the call's handle ends it, closing the window
//! ([`Event::FilesPickerClosed`]). The answer goes back as `file://` URIs.
//!
//! [`Event::FilesPickerClosed`]: nova_protocol::Event::FilesPickerClosed

use super::Shared;
use super::picker::Answer;
use crate::session_bus::{self, Stop};
use nova_protocol::{
    FILES_FILTER_MAX_PATTERNS, FILES_PATH_MAX_BYTES, FILES_PICKER_MAX_CHARS, FILES_PICKER_MAX_FILTERS, FileFilter,
    FilePicker, FilesWindow, PickMode, wire_text,
};
use std::ffi::OsString;
use std::future::poll_fn;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use zbus::wire::OwnedObjectPath;
use zbus::{Connection, DeserializeDict, SerializeDict, Type, interface};

pub const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.nova";
const PATH: &str = "/org/freedesktop/portal/desktop";
/// How long novad waits for the bus to answer.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// Most files one `SaveFiles` writes.
const MAX_SAVE_FILES: usize = 1024;
/// Most characters of patterns one filter keeps, so the filter fits in
/// the request Nova Files opens a folder with (REQUEST_MAX_BYTES), even
/// with every character escaped.
const FILTER_MAX_CHARS: usize = 8 * 1024;

/// The portal's response codes.
const RESPONSE_PICKED: u32 = 0;
const RESPONSE_CANCELLED: u32 = 1;
const RESPONSE_ENDED: u32 = 2;

/// A filter as the portal spells it: a name, and `(0, glob)` or `(1, MIME
/// type)` pairs.
type PortalFilter = (String, Vec<(u32, String)>);
const GLOB: u32 = 0;
const MIME: u32 = 1;

/// Serves the backend on a thread of its own until `stop`. Returns once
/// the name is claimed, or connecting failed (logged).
pub fn start(shared: &Arc<Shared>, stop: &Arc<Stop>) {
    let (ready, started) = std::sync::mpsc::channel();
    let serving = Arc::clone(shared);
    let stop = Arc::clone(stop);
    std::thread::spawn(move || {
        if let Err(e) = zbus::block_on(serve(&serving, &stop, &ready)) {
            serving.logger.warn(super::LOG_SOURCE, &format!("apps' Open and Save dialogs won't be Nova's: {e}"));
        }
        let _ = ready.send(());
    });
    // Waits as long as connecting may take; a timed-out call fails sooner.
    let _ = started.recv_timeout(2 * CALL_TIMEOUT);
}

async fn serve(shared: &Arc<Shared>, stop: &Stop, ready: &std::sync::mpsc::Sender<()>) -> zbus::Result<()> {
    let chooser = FileChooser { shared: Arc::clone(shared) };
    let connection = shared.sources.bus.builder().method_timeout(CALL_TIMEOUT).serve_at(PATH, chooser).build().await?;
    session_bus::claim(&connection, BUS_NAME).await?;
    shared.logger.info(super::LOG_SOURCE, &format!("serving apps' Open and Save dialogs as {BUS_NAME}"));
    let _ = ready.send(());
    poll_fn(|cx| stop.poll(cx)).await;
    drop(connection);
    Ok(())
}

/// The options of the three calls, as the portal passes them. Others are
/// left aside (`choices`: Nova's picker has no extra controls).
#[derive(Debug, Default, DeserializeDict, Type)]
#[zbus(signature = "a{sv}")]
struct Options {
    accept_label: Option<String>,
    multiple: Option<bool>,
    directory: Option<bool>,
    filters: Option<Vec<PortalFilter>>,
    current_filter: Option<PortalFilter>,
    current_name: Option<String>,
    /// NUL-terminated bytes, like the next two.
    current_folder: Option<Vec<u8>>,
    current_file: Option<Vec<u8>>,
    files: Option<Vec<Vec<u8>>>,
}

#[derive(Debug, Default, SerializeDict, Type)]
#[zbus(signature = "a{sv}")]
struct Results {
    uris: Option<Vec<String>>,
    current_filter: Option<PortalFilter>,
}

/// The interface the portal calls.
struct FileChooser {
    shared: Arc<Shared>,
}

impl FileChooser {
    /// Shows a picker for `app` as `mode` and waits for its answer.
    async fn choose(
        &self,
        connection: &Connection,
        handle: OwnedObjectPath,
        app: &str,
        title: String,
        mode: PickMode,
        options: Options,
    ) -> (u32, Results) {
        // The answer needs a URI for each file, in order, so a list Nova
        // can't take whole is turned down.
        let files = options.files.as_deref().filter(|_| mode == PickMode::SaveFiles).unwrap_or_default();
        if files.len() > MAX_SAVE_FILES || files.iter().any(|file| file_name(file.clone()).is_none()) {
            self.shared.logger.warn(super::LOG_SOURCE, "turned down saving files Nova can't name");
            return (RESPONSE_ENDED, Results::default());
        }
        let last_folder = self.shared.pickers.last_folder(app);
        let asked = Asked::new(mode, title, options, last_folder, &self.shared.sources.home);
        let files = asked.files.clone();
        let Some((picker, slot)) = self.shared.pickers.ask(app, asked.picker, files) else {
            self.shared.logger.warn(super::LOG_SOURCE, "too many file pickers are open; turned one down");
            return (RESPONSE_ENDED, Results::default());
        };
        let id = picker.id;
        // Served before the window opens, so a close always finds it; one
        // that comes first drops the window (Shared::open).
        let request = Request { shared: Arc::clone(&self.shared), picker: id };
        let served = connection.object_server().at(&handle, request).await;
        if let Err(e) = &served {
            self.shared.logger.warn(super::LOG_SOURCE, &format!("the app can't close its file picker: {e}"));
        }
        let window = FilesWindow { folder: wire_text(&asked.folder), select: asked.select, picker: Some(picker) };
        if let Err(e) = self.shared.open(window) {
            self.shared.logger.warn(super::LOG_SOURCE, &format!("couldn't show a file picker: {e}"));
            self.shared.pickers.end(id);
        }
        let answer = slot.wait().await;
        if served.is_ok() {
            let _ = connection.object_server().remove::<Request, _>(&handle).await;
        }
        match answer {
            Answer::Picked { paths, filter } => {
                let uris = paths.iter().map(|path| super::target::uri(path)).collect();
                let current_filter = filter.and_then(|filter| asked.filters.get(filter as usize).cloned());
                (RESPONSE_PICKED, Results { uris: Some(uris), current_filter })
            }
            Answer::Cancelled => (RESPONSE_CANCELLED, Results::default()),
            Answer::Ended => (RESPONSE_ENDED, Results::default()),
        }
    }
}

#[interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooser {
    async fn open_file(
        &self,
        #[zbus(connection)] connection: &Connection,
        handle: OwnedObjectPath,
        app_id: String,
        _parent_window: String,
        title: String,
        options: Options,
    ) -> (u32, Results) {
        self.choose(connection, handle, &app_id, title, PickMode::Open, options).await
    }

    async fn save_file(
        &self,
        #[zbus(connection)] connection: &Connection,
        handle: OwnedObjectPath,
        app_id: String,
        _parent_window: String,
        title: String,
        options: Options,
    ) -> (u32, Results) {
        self.choose(connection, handle, &app_id, title, PickMode::Save, options).await
    }

    async fn save_files(
        &self,
        #[zbus(connection)] connection: &Connection,
        handle: OwnedObjectPath,
        app_id: String,
        _parent_window: String,
        title: String,
        options: Options,
    ) -> (u32, Results) {
        self.choose(connection, handle, &app_id, title, PickMode::SaveFiles, options).await
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        // The interface's version this backend follows.
        4
    }
}

/// `org.freedesktop.impl.portal.Request` at a call's handle: the app gave
/// up asking.
struct Request {
    shared: Arc<Shared>,
    picker: u32,
}

#[interface(name = "org.freedesktop.impl.portal.Request")]
impl Request {
    fn close(&self) {
        self.shared.end_picker(self.picker);
    }
}

/// A call turned into Nova's picker, with what's needed to answer it.
struct Asked {
    picker: FilePicker,
    /// Where the window opens.
    folder: PathBuf,
    /// What it selects there: the file a save starts from.
    select: Vec<String>,
    /// The names `SaveFiles` writes.
    files: Vec<OsString>,
    /// The app's filters as it sent them, for the answer's
    /// `current_filter`, one for each of the picker's.
    filters: Vec<PortalFilter>,
}

impl Asked {
    /// The picker for `options`, opening where the app asked, else where
    /// it last picked (`last_folder`), else in `home`.
    fn new(mode: PickMode, title: String, options: Options, last_folder: Option<PathBuf>, home: &Path) -> Self {
        let current_file = options.current_file.and_then(bytes_path).filter(|_| mode == PickMode::Save);
        let asked_folder = options.current_folder.and_then(bytes_path).filter(|folder| folder.is_dir());
        let file_folder = current_file.as_ref().and_then(|file| file.parent()).filter(|folder| folder.is_dir());
        let folder = asked_folder
            .or_else(|| file_folder.map(Path::to_path_buf))
            .or(last_folder)
            .unwrap_or_else(|| home.to_path_buf());
        let name = current_file
            .as_ref()
            .and_then(|file| file.file_name())
            .map(wire_text)
            .or(options.current_name)
            .filter(|_| mode == PickMode::Save)
            .unwrap_or_default();
        let select =
            current_file.filter(|file| file.parent() == Some(folder.as_path())).map(wire_text).into_iter().collect();
        let (filters, filter) = kept_filters(options.filters.unwrap_or_default(), options.current_filter);
        let nova_filters = filters.iter().map(nova_filter).collect();
        let files = match mode {
            PickMode::SaveFiles => options.files.unwrap_or_default().into_iter().filter_map(file_name).collect(),
            PickMode::Open | PickMode::Save => Vec::new(),
        };
        let picker = FilePicker {
            id: 0,
            mode,
            title: capped(title),
            accept: capped(options.accept_label.unwrap_or_default()),
            multiple: mode == PickMode::Open && options.multiple.unwrap_or(false),
            folders: mode == PickMode::Open && options.directory.unwrap_or(false),
            name: capped(name),
            filters: nova_filters,
            filter,
        };
        Self { picker, folder, select, files, filters }
    }
}

/// The app's filters Nova keeps (at most [`FILES_PICKER_MAX_FILTERS`]), as
/// the app sent them, and the one to show first: the app's current one,
/// which is added when it isn't among them.
fn kept_filters(filters: Vec<PortalFilter>, current: Option<PortalFilter>) -> (Vec<PortalFilter>, u32) {
    let mut kept: Vec<PortalFilter> = filters.into_iter().take(FILES_PICKER_MAX_FILTERS).collect();
    let index = match current {
        Some(current) => match kept.iter().position(|filter| *filter == current) {
            Some(index) => index,
            None if kept.len() < FILES_PICKER_MAX_FILTERS => {
                kept.push(current);
                kept.len() - 1
            }
            None => 0,
        },
        None => 0,
    };
    (kept, u32::try_from(index).unwrap_or(0))
}

/// The app's filter as Nova's: its name cut to fit, and its globs and
/// types that fit, up to [`FILES_FILTER_MAX_PATTERNS`] and
/// [`FILTER_MAX_CHARS`] in all.
fn nova_filter((name, patterns): &PortalFilter) -> FileFilter {
    let fits = |(kind, pattern): &&(u32, String)| {
        matches!(*kind, GLOB | MIME) && pattern.chars().count() <= FILES_PICKER_MAX_CHARS
    };
    let mut chars = 0;
    let kept: Vec<&(u32, String)> = patterns
        .iter()
        .filter(fits)
        .take(FILES_FILTER_MAX_PATTERNS)
        .take_while(|(_, pattern)| {
            chars += pattern.chars().count();
            chars <= FILTER_MAX_CHARS
        })
        .collect();
    let of =
        |wanted: u32| kept.iter().filter(|(kind, _)| *kind == wanted).map(|(_, pattern)| pattern.clone()).collect();
    FileFilter { name: capped(name.clone()), globs: of(GLOB), mimes: of(MIME) }
}

/// A path the portal sent as NUL-terminated bytes, when it's absolute and
/// not too long.
fn bytes_path(mut bytes: Vec<u8>) -> Option<PathBuf> {
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    let path = PathBuf::from(OsString::from_vec(bytes));
    (path.is_absolute() && path.as_os_str().len() <= FILES_PATH_MAX_BYTES).then_some(path)
}

/// The name of a file `SaveFiles` writes: its last part, never a path.
fn file_name(bytes: Vec<u8>) -> Option<OsString> {
    let mut bytes = bytes;
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    let path = PathBuf::from(OsString::from_vec(bytes));
    path.file_name().map(ToOwned::to_owned).filter(|name| name.len() <= FILES_PATH_MAX_BYTES)
}

/// `text`, cut to [`FILES_PICKER_MAX_CHARS`].
fn capped(text: String) -> String {
    if text.chars().count() <= FILES_PICKER_MAX_CHARS {
        text
    } else {
        text.chars().take(FILES_PICKER_MAX_CHARS).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::bus_fixture::{Fixture, WAIT};
    use crate::hub::next_named;
    use std::collections::HashMap;
    use std::sync::mpsc;
    use zbus::Variant;
    use zbus::wire::OwnedVariant;

    type Reply = (u32, HashMap<String, OwnedVariant>);
    const HANDLE: &str = "/org/freedesktop/portal/desktop/request/1_42/nova1";

    /// An app's call of `method` through the portal, on a thread of its
    /// own since it waits for the picker. Its reply comes on the channel.
    fn ask(
        fixture: &Fixture,
        method: &'static str,
        options: HashMap<&'static str, Variant<'static>>,
    ) -> mpsc::Receiver<Reply> {
        let app = fixture.bus.connect();
        let (sender, replies) = mpsc::channel();
        std::thread::spawn(move || {
            let handle = zbus::wire::ObjectPath::try_from(HANDLE).unwrap();
            let body = (handle, "org.mozilla.firefox", "", "Upload", options);
            let call =
                app.call_method(Some(BUS_NAME), PATH, Some("org.freedesktop.impl.portal.FileChooser"), method, &body);
            let reply = zbus::block_on(call).unwrap();
            let _ = sender.send(reply.body().deserialize::<Reply>().unwrap());
        });
        replies
    }

    fn uris(reply: &Reply) -> Vec<String> {
        Vec::<String>::try_from(reply.1["uris"].try_clone().unwrap()).unwrap()
    }

    fn picker_id(window: &serde_json::Value) -> u32 {
        u32::try_from(window["picker"]["id"].as_u64().unwrap()).unwrap()
    }

    #[test]
    fn open_file_shows_a_picker_and_answers_with_the_picked_uris() {
        let fixture = Fixture::new("portal-open");
        let (client, nova_files, _stream) = fixture.nova_files_client();
        let images: PortalFilter = ("Images".into(), vec![(GLOB, "*.png".into()), (MIME, "image/jpeg".into())]);
        let options = HashMap::from([
            ("multiple", Variant::from(true)),
            ("filters", Variant::from(vec![("All".to_string(), vec![(GLOB, "*".to_string())]), images.clone()])),
            ("current_filter", Variant::from(images.clone())),
            ("current_folder", Variant::from(format!("{}\0", fixture.path("Downloads")).into_bytes())),
        ]);
        let replies = ask(&fixture, "OpenFile", options);
        let window = next_named(&nova_files, "files_window", WAIT);
        assert_eq!(window["folder"], fixture.path("Downloads"));
        let picker = &window["picker"];
        assert_eq!(picker["mode"], "open");
        assert_eq!(picker["title"], "Upload");
        assert_eq!(picker["multiple"], true);
        assert_eq!(picker["filter"], 1);
        assert_eq!(
            picker["filters"][1],
            serde_json::json!({"name": "Images", "globs": ["*.png"], "mimes": ["image/jpeg"]})
        );
        let report = nova_protocol::wire_text(fixture.dir.join("Downloads/report one.pdf"));
        fixture.files.pick(client, picker_id(&window), &[report], Some(1), false).unwrap();
        let reply = replies.recv_timeout(WAIT).unwrap();
        assert_eq!(reply.0, RESPONSE_PICKED);
        assert_eq!(uris(&reply), [format!("file://{}/report%20one.pdf", fixture.path("Downloads"))]);
        let shown = PortalFilter::try_from(reply.1["current_filter"].try_clone().unwrap()).unwrap();
        assert_eq!(shown, images);
    }

    #[test]
    fn save_file_starts_from_the_apps_name_and_the_last_folder() {
        let fixture = Fixture::new("portal-save");
        let (client, nova_files, _stream) = fixture.nova_files_client();
        // Firefox picked in Downloads before, so its save opens there.
        let replies = ask(&fixture, "OpenFile", HashMap::new());
        let window = next_named(&nova_files, "files_window", WAIT);
        assert_eq!(window["folder"], nova_protocol::wire_text(&*fixture.dir), "a first picker opens at home");
        let report = nova_protocol::wire_text(fixture.dir.join("Downloads/report one.pdf"));
        fixture.files.pick(client, picker_id(&window), &[report], None, false).unwrap();
        replies.recv_timeout(WAIT).unwrap();

        let replies = ask(&fixture, "SaveFile", HashMap::from([("current_name", Variant::from("cover.png"))]));
        let window = next_named(&nova_files, "files_window", WAIT);
        assert_eq!(window["folder"], fixture.path("Downloads"));
        assert_eq!(window["picker"]["mode"], "save");
        assert_eq!(window["picker"]["name"], "cover.png");
        let id = picker_id(&window);
        let folder = nova_protocol::wire_text(fixture.dir.join("Downloads"));
        assert!(
            fixture.files.pick(client, id, std::slice::from_ref(&folder), None, false).is_err(),
            "a folder isn't a file to save"
        );
        fixture.files.pick(client, id, &[format!("{folder}/cover.png")], None, false).unwrap();
        let reply = replies.recv_timeout(WAIT).unwrap();
        assert_eq!(uris(&reply), [format!("file://{}/cover.png", fixture.path("Downloads"))]);
    }

    #[test]
    fn save_files_writes_each_name_into_the_picked_folder() {
        let fixture = Fixture::new("portal-save-files");
        let (client, nova_files, _stream) = fixture.nova_files_client();
        let names = vec![b"a.txt\0".to_vec(), b"../b.txt\0".to_vec()];
        let replies = ask(&fixture, "SaveFiles", HashMap::from([("files", Variant::from(names))]));
        let window = next_named(&nova_files, "files_window", WAIT);
        assert_eq!(window["picker"]["mode"], "save_files");
        let folder = nova_protocol::wire_text(fixture.dir.join("Downloads"));
        fixture.files.pick(client, picker_id(&window), &[folder], None, false).unwrap();
        let reply = replies.recv_timeout(WAIT).unwrap();
        let downloads = fixture.path("Downloads");
        assert_eq!(uris(&reply), [format!("file://{downloads}/a.txt"), format!("file://{downloads}/b.txt")]);
    }

    #[test]
    fn cancelling_or_the_app_closing_ends_the_call() {
        let fixture = Fixture::new("portal-cancel");
        let (client, nova_files, _stream) = fixture.nova_files_client();
        let replies = ask(&fixture, "OpenFile", HashMap::new());
        let window = next_named(&nova_files, "files_window", WAIT);
        fixture.files.cancel_pick(client, picker_id(&window));
        assert_eq!(replies.recv_timeout(WAIT).unwrap().0, RESPONSE_CANCELLED);

        let replies = ask(&fixture, "OpenFile", HashMap::new());
        let window = next_named(&nova_files, "files_window", WAIT);
        let portal = fixture.bus.connect();
        let close =
            portal.call_method(Some(BUS_NAME), HANDLE, Some("org.freedesktop.impl.portal.Request"), "Close", &());
        zbus::block_on(close).unwrap();
        assert_eq!(
            next_named(&nova_files, "files_picker_closed", WAIT),
            serde_json::json!({"picker": picker_id(&window)})
        );
        assert_eq!(replies.recv_timeout(WAIT).unwrap().0, RESPONSE_ENDED);
    }

    #[test]
    fn nova_files_going_away_ends_its_pickers() {
        let fixture = Fixture::new("portal-gone");
        let (client, nova_files, _stream) = fixture.nova_files_client();
        let replies = ask(&fixture, "OpenFile", HashMap::new());
        next_named(&nova_files, "files_window", WAIT);
        fixture.files.client_gone(client);
        assert_eq!(replies.recv_timeout(WAIT).unwrap().0, RESPONSE_ENDED);
        // One waiting for the next Nova Files stays, and the module
        // stopping ends it.
        let replies = ask(&fixture, "OpenFile", HashMap::new());
        assert!(replies.recv_timeout(WAIT / 5).is_err());
        fixture.files.stop();
        assert_eq!(replies.recv_timeout(WAIT).unwrap().0, RESPONSE_ENDED);
    }

    #[test]
    fn filters_are_capped_and_the_current_one_added() {
        let many: Vec<PortalFilter> =
            (0..FILES_PICKER_MAX_FILTERS + 3).map(|n| (format!("F{n}"), vec![(GLOB, "*".into())])).collect();
        let (kept, index) = kept_filters(many, None);
        assert_eq!((kept.len(), index), (FILES_PICKER_MAX_FILTERS, 0));
        let odd: PortalFilter = ("Odd".into(), vec![(7, "x".into()), (GLOB, "*.odd".into())]);
        let (kept, index) = kept_filters(vec![], Some(odd.clone()));
        assert_eq!((kept, index), (vec![odd.clone()], 0), "kept as the app sent it, for the answer");
        let nova = nova_filter(&odd);
        assert_eq!((nova.globs, nova.mimes), (vec!["*.odd".to_string()], vec![]), "an unknown kind is left out");
        let long: PortalFilter = (
            "Long".into(),
            (0..FILES_FILTER_MAX_PATTERNS).map(|_| (GLOB, "*".repeat(FILES_PICKER_MAX_CHARS))).collect(),
        );
        let kept = nova_filter(&long).globs.len();
        assert_eq!(kept, FILTER_MAX_CHARS / FILES_PICKER_MAX_CHARS, "patterns stop at the filter's budget");
    }
}
