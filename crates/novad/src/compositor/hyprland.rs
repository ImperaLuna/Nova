//! Hyprland, through the two sockets in its instance folder
//! (`$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE`): `.socket2.sock`
//! streams events as `name>>data` lines, and `.socket.sock` answers one
//! request per connection (`j/<query>` answers in JSON).
//!
//! Hyprland's events don't say everything (a new workspace's monitor, for
//! one), so a workspace or monitor event makes novad ask for the monitors
//! and workspaces again; opening, moving, focusing or fullscreening a window
//! asks for the windows (a focus event also comes when a window changes its class), and
//! a layout switch asks for the keyboards (the main keyboard changes when
//! one is unplugged). Titles, focus and urgency come from the events alone.
//! Events that arrive together are answered with one round of questions.

use super::Backend;
use nova_protocol::{Capability, Desktop, FocusedWindow, Output, Workspace as NovaWorkspace, WorkspaceId};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const INSTANCE_VAR: &str = "HYPRLAND_INSTANCE_SIGNATURE";
const NAME: &str = "Hyprland";
const CAPABILITIES: &[Capability] = &[
    Capability::Workspaces,
    Capability::FocusWorkspace,
    Capability::FocusedWindow,
    Capability::KeyboardLayout,
    Capability::LogOut,
    Capability::Fullscreen,
];
const EVENTS_SOCKET: &str = ".socket2.sock";
const REQUEST_SOCKET: &str = ".socket.sock";
/// Between an event's name and its data.
const EVENT_SEPARATOR: &str = ">>";
/// Special (scratchpad) workspaces' names start with this. They aren't
/// shown on the bar.
const SPECIAL_PREFIX: &str = "special:";
/// JSON window addresses have it; event addresses don't.
const ADDRESS_PREFIX: &str = "0x";
/// The bit of a window's `fullscreen` mode that means fullscreen; the
/// other (1) is maximized, which keeps the bar's space.
const FULLSCREEN_MODE: u8 = 2;
/// How long Hyprland may take to answer a request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// The questions novad asks.
const MONITORS: &str = "j/monitors";
const WORKSPACES: &str = "j/workspaces";
const CLIENTS: &str = "j/clients";
const ACTIVE_WINDOW: &str = "j/activewindow";
const DEVICES: &str = "j/devices";

/// The instance folder of the Hyprland that set `instance`.
pub fn socket_dir(runtime_dir: &Path, instance: &std::ffi::OsStr) -> PathBuf {
    runtime_dir.join("hypr").join(instance)
}

pub struct Hyprland {
    dir: PathBuf,
}

impl Hyprland {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Sends one request on its own connection and returns the answer.
    fn request(&self, request: &str) -> Result<String, String> {
        let path = self.dir.join(REQUEST_SOCKET);
        let mut stream =
            UnixStream::connect(&path).map_err(|e| format!("can't reach Hyprland at {}: {e}", path.display()))?;
        stream.set_read_timeout(Some(REPLY_TIMEOUT)).map_err(|e| e.to_string())?;
        stream.write_all(request.as_bytes()).map_err(|e| format!("can't send to Hyprland: {e}"))?;
        // Hyprland reads the request until the client stops writing.
        let _ = stream.shutdown(Shutdown::Write);
        let mut answer = String::new();
        stream.read_to_string(&mut answer).map_err(|e| format!("Hyprland didn't answer {request}: {e}"))?;
        Ok(answer)
    }
}

impl Backend for Hyprland {
    fn name(&self) -> &'static str {
        NAME
    }

    fn capabilities(&self) -> &'static [Capability] {
        CAPABILITIES
    }

    fn watch(&self, report: &mut dyn FnMut(Desktop)) -> Result<(), String> {
        let path = self.dir.join(EVENTS_SOCKET);
        let stream =
            UnixStream::connect(&path).map_err(|e| format!("can't reach Hyprland at {}: {e}", path.display()))?;
        // Connected first, so nothing that changes while novad asks is missed.
        let mut ask = |request: &str| self.request(request);
        let mut state = State::load(&mut ask)?;
        report(state.desktop());
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).map_err(|e| format!("can't read Hyprland's events: {e}"))? == 0 {
                return Ok(());
            }
            state.apply(line.trim_end_matches('\n'));
            if reader.buffer().is_empty() {
                state.catch_up(&mut ask)?;
                report(state.desktop());
            }
        }
    }

    fn focus_workspace(&self, workspace: &NovaWorkspace) -> Result<(), String> {
        // A bare number is an id; named workspaces (negative ids) go by name.
        let target = if workspace.id > 0 { workspace.id.to_string() } else { format!("name:{}", workspace.name) };
        match self.request(&format!("dispatch workspace {target}"))?.trim() {
            "ok" => Ok(()),
            error => Err(format!("Hyprland: {error}")),
        }
    }

    fn log_out(&self) -> Result<(), String> {
        match self.request("dispatch exit")?.trim() {
            "ok" => Ok(()),
            error => Err(format!("Hyprland: {error}")),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Monitor {
    name: String,
    focused: bool,
    #[serde(rename = "activeWorkspace")]
    active_workspace: WorkspaceRef,
}

#[derive(Debug, Deserialize)]
struct WorkspaceRef {
    id: WorkspaceId,
}

#[derive(Debug, Deserialize)]
struct Workspace {
    id: WorkspaceId,
    name: String,
    monitor: String,
}

#[derive(Debug, Deserialize)]
struct Client {
    address: String,
    class: String,
    title: String,
    workspace: WorkspaceRef,
    /// 0 none, 1 maximized, 2 fullscreen, 3 both.
    fullscreen: u8,
}

#[derive(Debug, Deserialize)]
struct ActiveWindow {
    /// Missing when no window has focus (Hyprland answers `{}`).
    address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Devices {
    keyboards: Vec<Keyboard>,
}

#[derive(Debug, Deserialize)]
struct Keyboard {
    main: bool,
    active_keymap: String,
}

#[derive(Debug)]
struct Window {
    app_id: String,
    title: String,
    workspace: WorkspaceId,
    fullscreen: bool,
}

/// Hyprland's state as novad knows it. Window addresses are kept without
/// `0x`, as events write them.
#[derive(Debug)]
pub(super) struct State {
    monitors: Vec<Monitor>,
    workspaces: Vec<Workspace>,
    windows: HashMap<String, Window>,
    active_window: Option<String>,
    urgent: HashSet<String>,
    /// The layout of Hyprland's main keyboard.
    layout: Option<String>,
    /// What to ask again once the events that arrived together are applied.
    stale_workspaces: bool,
    stale_windows: bool,
    stale_layout: bool,
}

impl State {
    pub(super) fn load(ask: &mut impl FnMut(&str) -> Result<String, String>) -> Result<Self, String> {
        let active: ActiveWindow = query(ask, ACTIVE_WINDOW)?;
        let mut state = Self {
            monitors: Vec::new(),
            workspaces: Vec::new(),
            windows: HashMap::new(),
            active_window: active.address.map(|address| strip_address(&address).to_owned()),
            urgent: HashSet::new(),
            layout: None,
            stale_workspaces: true,
            stale_windows: true,
            stale_layout: true,
        };
        state.catch_up(ask)?;
        Ok(state)
    }

    /// Applies one event line.
    pub(super) fn apply(&mut self, line: &str) {
        let Some((event, data)) = line.split_once(EVENT_SEPARATOR) else { return };
        match event {
            "workspacev2" | "focusedmonv2" | "createworkspacev2" | "destroyworkspacev2" | "moveworkspacev2"
            | "renameworkspace" | "monitoraddedv2" | "monitorremovedv2" | "configreloaded" => {
                self.stale_workspaces = true;
            }
            "openwindow" | "movewindowv2" | "fullscreen" => self.stale_windows = true,
            "closewindow" => {
                self.windows.remove(data);
                self.urgent.remove(data);
            }
            "windowtitlev2" => {
                if let Some((address, title)) = data.split_once(',')
                    && let Some(window) = self.windows.get_mut(address)
                {
                    title.clone_into(&mut window.title);
                }
            }
            "activewindowv2" => {
                // Empty (or `,` in some releases) when nothing has focus.
                let address = data.trim_matches(',');
                self.urgent.remove(address);
                self.active_window = (!address.is_empty()).then(|| address.to_owned());
                self.stale_windows = true;
            }
            "urgent" => {
                if self.active_window.as_deref() != Some(data) {
                    self.urgent.insert(data.to_owned());
                }
            }
            // One per keyboard; which is the main one is in the devices.
            "activelayout" => self.stale_layout = true,
            _ => {}
        }
    }

    /// Asks Hyprland for whatever the events left unclear.
    pub(super) fn catch_up(&mut self, ask: &mut impl FnMut(&str) -> Result<String, String>) -> Result<(), String> {
        if std::mem::take(&mut self.stale_workspaces) {
            self.monitors = query(ask, MONITORS)?;
            self.workspaces = query(ask, WORKSPACES)?;
        }
        if std::mem::take(&mut self.stale_windows) {
            let clients: Vec<Client> = query(ask, CLIENTS)?;
            self.windows = clients
                .into_iter()
                .map(|client| {
                    let window = Window {
                        app_id: client.class,
                        title: client.title,
                        workspace: client.workspace.id,
                        fullscreen: client.fullscreen & FULLSCREEN_MODE != 0,
                    };
                    (strip_address(&client.address).to_owned(), window)
                })
                .collect();
            self.urgent.retain(|address| self.windows.contains_key(address));
        }
        if std::mem::take(&mut self.stale_layout) {
            let devices: Devices = query(ask, DEVICES)?;
            let keyboards = &devices.keyboards;
            let main = keyboards.iter().find(|keyboard| keyboard.main).or(keyboards.first());
            self.layout = main.map(|keyboard| keyboard.active_keymap.clone());
        }
        Ok(())
    }

    pub(super) fn desktop(&self) -> Desktop {
        let urgent_workspaces: HashSet<WorkspaceId> =
            self.urgent.iter().filter_map(|address| self.windows.get(address)).map(|window| window.workspace).collect();
        let occupied: HashSet<WorkspaceId> = self.windows.values().map(|window| window.workspace).collect();
        let fullscreen: HashSet<WorkspaceId> =
            self.windows.values().filter(|window| window.fullscreen).map(|window| window.workspace).collect();
        let outputs = self
            .monitors
            .iter()
            .map(|monitor| {
                let mut workspaces: Vec<&Workspace> = self
                    .workspaces
                    .iter()
                    .filter(|workspace| {
                        workspace.monitor == monitor.name && !workspace.name.starts_with(SPECIAL_PREFIX)
                    })
                    .collect();
                // Numbered workspaces in order, then named ones (negative ids).
                workspaces.sort_by_key(|workspace| (workspace.id < 0, workspace.id.unsigned_abs()));
                Output {
                    name: monitor.name.clone(),
                    focused: monitor.focused,
                    workspaces: workspaces
                        .into_iter()
                        .map(|workspace| {
                            let active = workspace.id == monitor.active_workspace.id;
                            NovaWorkspace {
                                id: workspace.id,
                                name: workspace.name.clone(),
                                active,
                                focused: active && monitor.focused,
                                urgent: urgent_workspaces.contains(&workspace.id),
                                empty: !occupied.contains(&workspace.id),
                                fullscreen: fullscreen.contains(&workspace.id),
                            }
                        })
                        .collect(),
                }
            })
            .collect();
        Desktop {
            compositor: NAME.into(),
            capabilities: CAPABILITIES.to_vec(),
            outputs,
            focused_window: self
                .active_window
                .as_ref()
                .and_then(|address| self.windows.get(address))
                .map(|window| FocusedWindow { app_id: window.app_id.clone(), title: window.title.clone() }),
            keyboard_layout: self.layout.clone(),
        }
    }
}

fn query<T: serde::de::DeserializeOwned>(
    ask: &mut impl FnMut(&str) -> Result<String, String>,
    request: &str,
) -> Result<T, String> {
    let answer = ask(request)?;
    serde_json::from_str(&answer).map_err(|e| format!("Hyprland sent an odd answer to {request} ({e})"))
}

fn strip_address(address: &str) -> &str {
    address.strip_prefix(ADDRESS_PREFIX).unwrap_or(address)
}
