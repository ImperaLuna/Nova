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
//! - Opening files in apps is the shared `crate::apps::opening`, over the
//!   module's one copy of the MIME database.
//!
//! Its events go on [`Topic::Files`], which only the Nova Files process
//! subscribes to. A window asked for while no client is subscribed waits
//! until one subscribes, which then gets it ([`Files::send_state`]); asking
//! and subscribing take the same lock, so none is lost or sent twice.

mod file_manager1;
mod target;

use crate::apps::opening::Opener;
use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use crate::session_bus::{Bus, Stop};
use nova_protocol::{Event, FeatureState, FilesWindow, Topic};
use std::sync::{Arc, Mutex};

pub const FEATURE_ID: &str = "files";
const LOG_SOURCE: &str = "files";

/// Where Nova Files runs from.
#[derive(Clone)]
pub struct Sources {
    /// Where `org.freedesktop.FileManager1` is served.
    pub bus: Bus,
}

impl Sources {
    pub fn system() -> Self {
        Self { bus: Bus::Session }
    }
}

pub struct Files {
    shared: Arc<Shared>,
    /// Opens files in apps.
    opener: Arc<Opener>,
    /// Ends the FileManager1 service.
    stop: Arc<Stop>,
}

/// What the module's services share.
struct Shared {
    sources: Sources,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    /// Windows asked for that no Nova Files process has been sent yet.
    waiting: Mutex<Vec<FilesWindow>>,
}

impl Files {
    pub fn start(sources: Sources, hub: Arc<Hub>, logger: Arc<Logger>, opener: Opener) -> Arc<Self> {
        let shared = Arc::new(Shared { sources, hub, logger, waiting: Mutex::new(Vec::new()) });
        let stop = Arc::new(Stop::default());
        file_manager1::start(&shared, &stop);
        Arc::new(Self { shared, opener: Arc::new(opener), stop })
    }

    /// What opens files in apps.
    pub fn opener(&self) -> Arc<Opener> {
        Arc::clone(&self.opener)
    }

    /// Opens a window on `folder` with `select` selected, once both check
    /// out ([`target::window`]).
    pub fn show(&self, folder: &str, select: &[String]) -> Result<(), String> {
        self.shared.open(target::window(folder, select)?);
        Ok(())
    }

    /// Sends client `client`, which just subscribed, the windows waiting.
    pub fn send_state(&self, client: ClientId) {
        for window in lock(&self.shared.waiting).drain(..) {
            self.shared.hub.send_to(client, &Event::FilesWindow(window));
        }
    }

    /// Stops it: it publishes nothing more, gives up FileManager1, and
    /// windows still waiting are dropped.
    pub fn stop(&self) {
        self.stop.stop();
        lock(&self.shared.waiting).clear();
    }
}

impl Shared {
    /// Sends `window` to the Nova Files process, or keeps it for the next
    /// one to subscribe.
    fn open(&self, window: FilesWindow) {
        let mut waiting = lock(&self.waiting);
        if self.hub.publish(&Event::FilesWindow(window.clone())) == 0 {
            self.logger.info(LOG_SOURCE, &format!("{} waits for Nova Files to start", window.folder));
            waiting.push(window);
        }
    }
}

/// No options yet.
impl crate::modules::Module for Files {
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<Topic> = Some(Topic::Files);
    /// Files open in apps.
    const NEEDS_APPS: bool = true;
    type Sources = Sources;
    type Config = ();

    fn sources(_: &crate::modules::Env) -> Sources {
        Sources::system()
    }

    fn config(_: &crate::config::Config) {}

    fn start(ctx: &crate::modules::Context, sources: &Sources, (): &(), _: &[FeatureState]) -> Arc<Self> {
        let data_dirs = crate::data_dirs();
        let (mime, problems) = crate::mime::MimeDb::load(&data_dirs);
        for problem in problems {
            ctx.logger.warn(LOG_SOURCE, &problem);
        }
        let places = crate::apps::associations::Places::system(&data_dirs);
        let opener = Opener::new(Arc::new(mime), places, Arc::clone(&ctx.apps), &ctx.path_var, Arc::clone(&ctx.logger));
        Self::start(sources.clone(), Arc::clone(&ctx.hub), Arc::clone(&ctx.logger), opener)
    }

    fn send_state(&self, client: ClientId) {
        Self::send_state(self, client);
    }

    fn stop(&self) {
        Self::stop(self);
    }
}
