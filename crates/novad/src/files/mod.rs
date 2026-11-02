//! Nova Files (feature `files`): the file manager's daemon side. The
//! windows are their own Quickshell process, started on demand, and only
//! render; what they show and do is worked out here.
//!
//! One file per job, all under this folder:
//! - `mod.rs`: the module, and the windows asked for (`nova files`,
//!   [`Files::show`]) that the Nova Files process hasn't been sent yet.
//! - `target.rs`: what a window shows, checked: an absolute folder that
//!   exists, and items in it to select.
//!
//! Windows go only to the Nova Files process, once it has attached
//! (`attach_files`, [`Files::attach`]); no other client on [`Topic::Files`]
//! takes them. One asked for while none is attached waits, and goes to it
//! when it attaches. Asking, attaching and detaching take the same lock,
//! so none is lost or sent twice.

mod target;

use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use nova_protocol::{Event, FeatureState, FilesWindow, Topic};
use std::sync::{Arc, Mutex};

pub const FEATURE_ID: &str = "files";
const LOG_SOURCE: &str = "files";
/// More windows than anyone asks for before Nova Files starts; past it,
/// more are refused rather than kept.
const MAX_WAITING: usize = 64;

/// Where Nova Files runs from; nothing yet.
#[derive(Clone)]
pub struct Sources;

pub struct Files {
    shared: Arc<Shared>,
}

/// What the module's services share.
struct Shared {
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    delivery: Mutex<Delivery>,
}

/// Where windows go, under one lock.
#[derive(Default)]
struct Delivery {
    /// The Nova Files process's connection, while it's attached.
    attached: Option<ClientId>,
    /// Windows asked for that no Nova Files process has been sent yet.
    waiting: Vec<FilesWindow>,
}

impl Files {
    pub fn start(hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        Arc::new(Self { shared: Arc::new(Shared { hub, logger, delivery: Mutex::default() }) })
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
            let waiting = std::mem::take(&mut delivery.waiting);
            let mut unsent = waiting.into_iter().skip_while(|window| self.shared.send(client, window));
            if let Some(window) = unsent.next() {
                // It hung up already.
                delivery.attached = None;
                delivery.waiting = std::iter::once(window).chain(unsent).collect();
            }
        } else if delivery.attached == Some(client) {
            delivery.attached = None;
        }
    }

    /// Client `client` hung up: windows aren't sent to it any more.
    pub fn client_gone(&self, client: ClientId) {
        let mut delivery = lock(&self.shared.delivery);
        if delivery.attached == Some(client) {
            delivery.attached = None;
        }
    }

    /// Stops it: it sends nothing more, and drops the windows waiting.
    pub fn stop(&self) {
        *lock(&self.shared.delivery) = Delivery::default();
    }
}

impl Shared {
    /// Sends `window` to the Nova Files process, or keeps it for the next
    /// one to attach, unless [`MAX_WAITING`] already wait.
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
        Ok(())
    }

    fn send(&self, client: ClientId, window: &FilesWindow) -> bool {
        self.hub.send_to(client, &Event::FilesWindow(window.clone()))
    }
}

/// No options yet.
impl crate::modules::Module for Files {
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<Topic> = Some(Topic::Files);
    type Sources = Sources;
    type Config = ();

    fn sources(_: &crate::modules::Env) -> Sources {
        Sources
    }

    fn config(_: &crate::config::Config) {}

    fn start(ctx: &crate::modules::Context, _: &Sources, (): &(), _: &[FeatureState]) -> Arc<Self> {
        Self::start(Arc::clone(&ctx.hub), Arc::clone(&ctx.logger))
    }

    fn client_gone(&self, client: ClientId) {
        Self::client_gone(self, client);
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

    /// Files, and a folder to show.
    struct Fixture {
        dir: TempDir,
        hub: Arc<Hub>,
        files: Arc<Files>,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir = TempDir::new(&format!("files-{name}"));
            std::fs::create_dir_all(dir.join("Downloads")).unwrap();
            let hub = Arc::new(Hub::default());
            let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
            let files = Files::start(Arc::clone(&hub), logger);
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
