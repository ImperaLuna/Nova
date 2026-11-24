mod hyprland;
#[cfg(any(test, feature = "test-support"))]
pub use hyprland::LUA_PROBE as HYPRLAND_LUA_PROBE;
mod niri;

use nova_desktop_info::{Capability, Desktop, EVENT, Output, Step, TOPIC, Workspace, WorkspaceId};

use nova_base::backoff::Backoff;
use nova_base::lock;
use nova_carrier::topic::Single;
use nova_hub::{ClientId, Hub};
use nova_log::Logger;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const LOG_SOURCE: &str = "compositor";
const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(60);

pub trait Backend: Send + Sync {
    fn name(&self) -> &'static str;

    fn capabilities(&self) -> &'static [Capability];

    fn watch(&self, report: &mut dyn FnMut(Desktop)) -> Result<(), String>;

    fn focus_workspace(&self, workspace: &Workspace) -> Result<(), String>;

    fn focus_output(&self, output: &Output) -> Result<(), String>;

    fn log_out(&self) -> Result<(), String>;

    fn focus_window(&self, app_ids: &[String], here: &Workspace) -> Result<bool, String>;

    fn window_workspace(&self, app_ids: &[String], here: &Workspace) -> Result<Option<WorkspaceId>, String>;
    fn restores_session_lock(&self) -> Result<bool, String>;
}

pub fn detect(env: impl Fn(&str) -> Option<OsString>) -> Option<Box<dyn Backend>> {
    let set = |name: &str| env(name).filter(|value| !value.is_empty());
    if let Some(socket) = set(niri::SOCKET_VAR) {
        return Some(Box::new(niri::Niri::new(socket.into())));
    }
    let instance = set(hyprland::INSTANCE_VAR)?;
    let runtime_dir = PathBuf::from(set("XDG_RUNTIME_DIR")?);
    Some(Box::new(hyprland::Hyprland::new(hyprland::socket_dir(&runtime_dir, &instance))))
}

pub struct Compositor {
    backend: Box<dyn Backend>,
    desktop: Mutex<Option<Desktop>>,
    hub: Arc<Hub>,
    output_listeners: Mutex<Vec<OutputListener>>,
}

type OutputListener = Box<dyn Fn(Vec<String>) + Send>;

impl Compositor {
    pub fn start(backend: Box<dyn Backend>, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        logger.info(LOG_SOURCE, &format!("following {}", backend.name()));
        let compositor =
            Arc::new(Self { backend, desktop: Mutex::new(None), hub, output_listeners: Mutex::new(Vec::new()) });
        let watcher = Arc::clone(&compositor);
        std::thread::spawn(move || {
            let name = watcher.backend.name();
            let mut retry = Backoff::new(FIRST_RETRY, MAX_RETRY);
            loop {
                let mut reported = false;
                let result = watcher.backend.watch(&mut |desktop| {
                    reported = true;
                    watcher.update(desktop);
                });
                if reported {
                    retry.reset();
                }
                let wait = retry.next();
                match result {
                    Ok(()) => return logger.info(LOG_SOURCE, &format!("{name} closed its IPC connection")),
                    Err(e) => {
                        logger.warn(LOG_SOURCE, &format!("lost {name}, trying again in {}s: {e}", wait.as_secs()))
                    }
                }
                nova_base::backoff::wait_out(wait);
            }
        });
        compositor
    }

    pub fn desktop(&self) -> Option<Desktop> {
        lock(&self.desktop).clone()
    }

    pub fn focused_output(&self) -> Option<String> {
        let desktop = lock(&self.desktop);
        desktop.as_ref()?.outputs.iter().find(|output| output.focused).map(|output| output.name.clone())
    }

    // The listener runs with the desktop locked, so it must not call back into the compositor: https://linear.app/imperaluna/issue/IMP-168
    pub fn on_outputs(&self, listener: impl Fn(Vec<String>) + Send + 'static) {
        let desktop = lock(&self.desktop);
        if let Some(desktop) = &*desktop {
            listener(output_names(desktop).cloned().collect());
        }
        lock(&self.output_listeners).push(Box::new(listener));
    }

    pub fn subscribe(&self, client: ClientId) {
        let desktop = lock(&self.desktop);
        self.hub.subscribe(client, TOPIC);
        if let Some(desktop) = &*desktop {
            self.hub.send_to(client, &event(desktop));
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

    pub fn focus_app(&self, app_ids: &[String], output: Option<&str>) -> Result<bool, String> {
        let desktop = self.desktop().ok_or("the compositor hasn't said what it shows yet")?;
        self.backend.focus_window(app_ids, shown_on(&desktop, output)?)
    }

    pub fn app_workspace(&self, app_ids: &[String]) -> Result<Option<String>, String> {
        let desktop = self.desktop().ok_or("the compositor hasn't said what it shows yet")?;
        let Some(id) = self.backend.window_workspace(app_ids, shown_on(&desktop, None)?)? else { return Ok(None) };
        let workspace =
            desktop.outputs.iter().flat_map(|output| &output.workspaces).find(|workspace| workspace.id == id);
        Ok(workspace.map(|workspace| workspace.name.clone()))
    }

    pub fn focus_output(&self, output: &str) -> Result<(), String> {
        let desktop = self.desktop().ok_or("the compositor hasn't said what it shows yet")?;
        let output = desktop.outputs.iter().find(|candidate| candidate.name == output).ok_or("no such output")?;
        if output.focused || !self.can(Capability::FocusWorkspace) {
            return Ok(());
        }
        self.backend.focus_output(output)
    }

    pub fn can(&self, capability: Capability) -> bool {
        self.backend.capabilities().contains(&capability)
    }

    pub fn log_out(&self) -> Result<(), String> {
        self.require(Capability::LogOut)?;
        self.backend.log_out()
    }

    fn require(&self, capability: Capability) -> Result<(), String> {
        if self.can(capability) {
            return Ok(());
        }
        Err(format!("{} can't do that (no {capability:?} capability)", self.backend.name()))
    }

    fn update(&self, mut desktop: Desktop) {
        desktop.outputs.sort_by(|a, b| a.name.cmp(&b.name));
        let mut current = lock(&self.desktop);
        if current.as_ref() == Some(&desktop) {
            return;
        }
        self.hub.publish(&event(&desktop));
        if current.as_ref().is_none_or(|current| !output_names(current).eq(output_names(&desktop))) {
            let names: Vec<String> = output_names(&desktop).cloned().collect();
            for listener in lock(&self.output_listeners).iter() {
                listener(names.clone());
            }
        }
        *current = Some(desktop);
    }
}

pub fn event(desktop: &Desktop) -> Single<'_, Desktop> {
    Single::new(EVENT, TOPIC, desktop)
}

fn output_names(desktop: &Desktop) -> impl Iterator<Item = &String> {
    desktop.outputs.iter().map(|output| &output.name)
}

fn shown_on<'a>(desktop: &'a Desktop, output: Option<&str>) -> Result<&'a Workspace, String> {
    let output = match output {
        Some(name) => desktop.outputs.iter().find(|candidate| candidate.name == name),
        None => desktop.outputs.iter().find(|candidate| candidate.focused),
    }
    .ok_or("no such output")?;
    Ok(output.workspaces.iter().find(|workspace| workspace.active).ok_or("no workspace shows there")?)
}

fn neighbour(desktop: &Desktop, output: &str, step: Step) -> Result<Option<Workspace>, String> {
    let workspaces: Vec<&Workspace> = desktop
        .outputs
        .iter()
        .find(|candidate| candidate.name == output)
        .ok_or_else(|| format!("{} has no output {output:?}", desktop.compositor))?
        .workspaces
        .iter()
        .filter(|workspace| !workspace.special)
        .collect();
    let Some(active) = workspaces.iter().position(|workspace| workspace.active) else { return Ok(None) };
    let target = match step {
        Step::Next => active.checked_add(1),
        Step::Previous => active.checked_sub(1),
    };
    Ok(target.and_then(|index| workspaces.get(index)).map(|workspace| (*workspace).clone()))
}

#[cfg(any(test, feature = "test-support"))]
pub fn recorded_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recorded")
}

#[cfg(test)]
mod tests;
