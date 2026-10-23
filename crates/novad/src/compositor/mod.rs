//! The compositor: its outputs, workspaces, focused window and keyboard
//! layout as one [`Desktop`] on the `compositor` topic, the same shape on
//! every compositor, and the workspace actions the bar needs.
//!
//! Each compositor is a [`Backend`] that follows the compositor's own event
//! stream (no polling) and says which [`Capability`]s it has; the UI hides
//! what a compositor can't do instead of asking which compositor it is.
//! novad picks the backend from the variables the compositor sets.

mod hyprland;
mod niri;

use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use nova_protocol::{Capability, Desktop, Event, Step, Topic, Workspace, WorkspaceId};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const LOG_SOURCE: &str = "compositor";
/// How long to wait before following the compositor again after a failure,
/// doubling up to `MAX_RETRY` while it keeps failing.
const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(60);

/// One compositor's IPC.
pub trait Backend: Send + Sync {
    /// For people: `nova status` and the log.
    fn name(&self) -> &'static str;

    /// What [`Backend::watch`] reports and which actions work.
    fn capabilities(&self) -> &'static [Capability];

    /// Connects and calls `report` with the whole desktop, then again after
    /// every change, until the compositor closes the connection (`Ok`) or it
    /// fails.
    fn watch(&self, report: &mut dyn FnMut(Desktop)) -> Result<(), String>;

    /// Shows `workspace`, one the last report listed.
    fn focus_workspace(&self, workspace: &Workspace) -> Result<(), String>;
}

/// The backend for the compositor this session runs, from its variables in
/// `env`. `None` when it's none novad knows.
pub fn detect(env: impl Fn(&str) -> Option<OsString>) -> Option<Box<dyn Backend>> {
    let set = |name: &str| env(name).filter(|value| !value.is_empty());
    if let Some(socket) = set(niri::SOCKET_VAR) {
        return Some(Box::new(niri::Niri::new(socket.into())));
    }
    let instance = set(hyprland::INSTANCE_VAR)?;
    let runtime_dir = PathBuf::from(set("XDG_RUNTIME_DIR")?);
    Some(Box::new(hyprland::Hyprland::new(hyprland::socket_dir(&runtime_dir, &instance))))
}

/// The compositor's state, kept current by a thread that follows its events.
pub struct Compositor {
    backend: Box<dyn Backend>,
    /// The last report, `None` until the first. Locked while it's published
    /// and while a client subscribes, so a new subscriber never gets an
    /// older desktop after a newer one.
    desktop: Mutex<Option<Desktop>>,
    hub: Arc<Hub>,
}

impl Compositor {
    pub fn start(backend: Box<dyn Backend>, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        logger.info(LOG_SOURCE, &format!("following {}", backend.name()));
        let compositor = Arc::new(Self { backend, desktop: Mutex::new(None), hub });
        let watcher = Arc::clone(&compositor);
        std::thread::spawn(move || {
            let name = watcher.backend.name();
            // A failure (the compositor not up yet, a request that timed out)
            // starts over from a fresh connection, waiting longer each time.
            // The compositor closing the connection means it's gone.
            let mut retry = FIRST_RETRY;
            loop {
                let mut reported = false;
                let result = watcher.backend.watch(&mut |desktop| {
                    reported = true;
                    watcher.update(desktop);
                });
                match result {
                    Ok(()) => return logger.info(LOG_SOURCE, &format!("{name} closed its IPC connection")),
                    Err(e) => {
                        logger.warn(LOG_SOURCE, &format!("lost {name}, trying again in {}s: {e}", retry.as_secs()))
                    }
                }
                std::thread::sleep(retry);
                retry = if reported { FIRST_RETRY } else { (retry * 2).min(MAX_RETRY) };
            }
        });
        compositor
    }

    /// What the compositor shows now, once it has said.
    pub fn desktop(&self) -> Option<Desktop> {
        lock(&self.desktop).clone()
    }

    /// Subscribes `client` and sends it the current desktop, if known.
    pub fn subscribe(&self, client: ClientId) {
        let desktop = lock(&self.desktop);
        self.hub.subscribe(client, Topic::Compositor);
        if let Some(desktop) = &*desktop {
            self.hub.send_to(client, &Event::Compositor(desktop.clone()));
        }
    }

    pub fn focus_workspace(&self, id: WorkspaceId) -> Result<(), String> {
        self.require(Capability::FocusWorkspace)?;
        let workspace = self
            .desktop()
            .and_then(|desktop| desktop.outputs.into_iter().flat_map(|output| output.workspaces).find(|w| w.id == id))
            .ok_or_else(|| format!("{} has no workspace {id}", self.backend.name()))?;
        self.backend.focus_workspace(&workspace)
    }

    pub fn step_workspace(&self, output: &str, step: Step) -> Result<(), String> {
        self.require(Capability::FocusWorkspace)?;
        let desktop =
            self.desktop().ok_or_else(|| format!("{} hasn't reported its workspaces yet", self.backend.name()))?;
        match neighbour(&desktop, output, step)? {
            Some(workspace) => self.backend.focus_workspace(&workspace),
            None => Ok(()),
        }
    }

    fn require(&self, capability: Capability) -> Result<(), String> {
        if self.backend.capabilities().contains(&capability) {
            return Ok(());
        }
        Err(format!("{} can't do that (no {capability:?} capability)", self.backend.name()))
    }

    /// Publishes `desktop` when it differs from the last one.
    fn update(&self, mut desktop: Desktop) {
        desktop.outputs.sort_by(|a, b| a.name.cmp(&b.name));
        let mut current = lock(&self.desktop);
        if current.as_ref() == Some(&desktop) {
            return;
        }
        self.hub.publish(&Event::Compositor(desktop.clone()));
        *current = Some(desktop);
    }
}

/// The workspace after or before the active one on `output`; `None` at
/// either end.
fn neighbour(desktop: &Desktop, output: &str, step: Step) -> Result<Option<Workspace>, String> {
    let workspaces = &desktop
        .outputs
        .iter()
        .find(|candidate| candidate.name == output)
        .ok_or_else(|| format!("{} has no output {output:?}", desktop.compositor))?
        .workspaces;
    let Some(active) = workspaces.iter().position(|workspace| workspace.active) else { return Ok(None) };
    let target = match step {
        Step::Next => active.checked_add(1),
        Step::Previous => active.checked_sub(1),
    };
    Ok(target.and_then(|index| workspaces.get(index)).cloned())
}

#[cfg(test)]
mod tests;
