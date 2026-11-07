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
//!
//! Actions go out as dispatchers, in one of two languages: Hyprland with a
//! Lua config (all of 0.57, and 0.56 set up with `hyprland.lua`) runs
//! `dispatch <x>` as the Lua `return hl.dispatch(<x>)`, so it takes
//! `hl.dsp.exit()` where older setups take `exit`. novad asks once which
//! one this Hyprland speaks ([`Dialect`]).

use super::Backend;
use nova_protocol::{Capability, Desktop, FocusedWindow, Output, Workspace as NovaWorkspace, WorkspaceId};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
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
    Capability::SpecialWorkspaces,
];
const EVENTS_SOCKET: &str = ".socket2.sock";
const REQUEST_SOCKET: &str = ".socket.sock";
/// Between an event's name and its data.
const EVENT_SEPARATOR: &str = ">>";
/// Special (scratchpad) workspaces' names start with this; dispatchers
/// take the rest.
const SPECIAL_PREFIX: &str = "special:";
/// JSON window addresses have it; event addresses don't.
const ADDRESS_PREFIX: &str = "0x";
/// The bit of a window's `fullscreen` mode that means fullscreen; the
/// other (1) is maximized, which keeps the bar's space.
const FULLSCREEN_MODE: u8 = 2;
/// FNV-1a (64 bits), which turns an address into an id.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// Bits of an integer a JavaScript number holds exactly (beside its sign).
const JS_EXACT_BITS: u32 = 52;
/// How long Hyprland may take to answer a request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// The questions novad asks.
const MONITORS: &str = "j/monitors";
const WORKSPACES: &str = "j/workspaces";
const CLIENTS: &str = "j/clients";
const ACTIVE_WINDOW: &str = "j/activewindow";
const DEVICES: &str = "j/devices";

/// A Lua dispatcher that does nothing: only a Hyprland that speaks Lua
/// answers it with `ok` (others say "Invalid dispatcher").
pub(crate) const LUA_PROBE: &str = "dispatch hl.dsp.no_op()";
/// Hyprland's answer to a dispatcher that ran.
const DISPATCHED: &str = "ok";

/// The instance folder of the Hyprland that set `instance`.
pub fn socket_dir(runtime_dir: &Path, instance: &std::ffi::OsStr) -> PathBuf {
    runtime_dir.join("hypr").join(instance)
}

pub struct Hyprland {
    dir: PathBuf,
    /// Which dispatchers it takes, once asked. A Hyprland keeps its config
    /// language until it quits, and a new one comes with a new instance.
    dialect: Mutex<Option<Dialect>>,
}

impl Hyprland {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, dialect: Mutex::new(None) }
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

impl Hyprland {
    /// A window whose class (or first class) is one of `app_ids` (lower
    /// case), one on `here` first.
    fn app_window(&self, app_ids: &[String], here: &NovaWorkspace) -> Result<Option<Client>, String> {
        let mut matching: Vec<Client> = self
            .clients()?
            .into_iter()
            .filter(|client| {
                [&client.class, &client.initial_class].iter().any(|class| app_ids.contains(&class.to_lowercase()))
            })
            .collect();
        matching.sort_by_key(|client| client.workspace.id != here.id);
        Ok(matching.into_iter().next())
    }

    /// Every window, as Hyprland answers now.
    fn clients(&self) -> Result<Vec<Client>, String> {
        let answer = self.request(CLIENTS)?;
        serde_json::from_str(&answer).map_err(|e| format!("Hyprland sent odd windows ({e})"))
    }

    /// The dispatcher language this Hyprland takes, asked the first time.
    fn dialect(&self) -> Result<Dialect, String> {
        let mut known = self.dialect.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(dialect) = *known {
            return Ok(dialect);
        }
        let dialect = if self.request(LUA_PROBE)?.trim() == DISPATCHED { Dialect::Lua } else { Dialect::Hyprlang };
        *known = Some(dialect);
        Ok(dialect)
    }

    /// Runs `action` as one of Hyprland's dispatchers.
    fn dispatch(&self, action: &Action) -> Result<(), String> {
        let dispatcher = action.dispatcher(self.dialect()?);
        match self.request(&format!("dispatch {dispatcher}"))?.trim() {
            DISPATCHED => Ok(()),
            error => Err(format!("Hyprland: {error}")),
        }
    }
}

/// The language of Hyprland's dispatchers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialect {
    /// `exit`, `workspace 3`: Hyprland before Lua configs, or one still on
    /// `hyprland.conf`.
    Hyprlang,
    /// `hl.dsp.exit()`, `hl.dsp.focus({ workspace = "3" })`.
    Lua,
}

/// What novad asks Hyprland to do. Workspaces, monitors and windows are
/// named by the same selectors (`name:web`, `address:0x…`) in both dialects.
#[derive(Debug)]
enum Action {
    FocusWorkspace(String),
    /// Shows or hides the special workspace of this name (without `special:`).
    ToggleSpecial(String),
    FocusMonitor(String),
    FocusWindow(String),
    Exit,
}

impl Action {
    fn dispatcher(&self, dialect: Dialect) -> String {
        match dialect {
            Dialect::Hyprlang => match self {
                Self::FocusWorkspace(workspace) => format!("workspace {workspace}"),
                Self::ToggleSpecial(name) => format!("togglespecialworkspace {name}"),
                Self::FocusMonitor(monitor) => format!("focusmonitor {monitor}"),
                Self::FocusWindow(window) => format!("focuswindow {window}"),
                Self::Exit => "exit".into(),
            },
            Dialect::Lua => match self {
                Self::FocusWorkspace(workspace) => format!("hl.dsp.focus({{ workspace = {} }})", lua_string(workspace)),
                Self::ToggleSpecial(name) => format!("hl.dsp.workspace.toggle_special({})", lua_string(name)),
                Self::FocusMonitor(monitor) => format!("hl.dsp.focus({{ monitor = {} }})", lua_string(monitor)),
                Self::FocusWindow(window) => format!("hl.dsp.focus({{ window = {} }})", lua_string(window)),
                Self::Exit => "hl.dsp.exit()".into(),
            },
        }
    }
}

/// `text` as a quoted Lua string. Workspace names are the user's, so quotes,
/// backslashes and control characters are escaped (`\ddd` takes three digits,
/// so a digit after it stays a digit).
fn lua_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            c if c.is_ascii_control() => quoted.push_str(&format!("\\{:03}", c as u8)),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// How dispatchers name `workspace`: a bare number is an id; named
/// workspaces (negative ids) go by name.
fn workspace_target(workspace: &NovaWorkspace) -> String {
    if workspace.id > 0 { workspace.id.to_string() } else { format!("name:{}", workspace.name) }
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
        if workspace.special {
            return self.dispatch(&Action::ToggleSpecial(workspace.name.clone()));
        }
        self.dispatch(&Action::FocusWorkspace(workspace_target(workspace)))
    }

    fn focus_output(&self, output: &Output) -> Result<(), String> {
        // Not by workspace: the one under a special workspace shown there
        // isn't active, and showing the special one again hides it.
        self.dispatch(&Action::FocusMonitor(output.name.clone()))
    }

    fn focus_window(&self, app_ids: &[String], here: &NovaWorkspace) -> Result<bool, String> {
        let Some(client) = self.app_window(app_ids, here)? else { return Ok(false) };
        // Hyprland shows the window's workspace and monitor for it.
        self.dispatch(&Action::FocusWindow(format!("address:{}", client.address)))?;
        Ok(true)
    }

    fn focus_window_id(&self, id: &str) -> Result<bool, String> {
        // Desktop ids are written without `0x`, as events write them.
        let clients = self.clients()?;
        let Some(client) = clients.iter().find(|client| strip_address(&client.address) == strip_address(id)) else {
            return Ok(false);
        };
        self.dispatch(&Action::FocusWindow(format!("address:{}", client.address)))?;
        Ok(true)
    }

    fn window_workspace(&self, app_ids: &[String], here: &NovaWorkspace) -> Result<Option<WorkspaceId>, String> {
        Ok(self.app_window(app_ids, here)?.map(|client| client.workspace.id))
    }

    fn log_out(&self) -> Result<(), String> {
        self.dispatch(&Action::Exit)
    }
}

#[derive(Debug, Deserialize)]
struct Monitor {
    name: String,
    focused: bool,
    #[serde(rename = "activeWorkspace")]
    active_workspace: WorkspaceRef,
    /// The special workspace showing over it; id 0 when none.
    #[serde(rename = "specialWorkspace", default)]
    special_workspace: SpecialWorkspaceRef,
}

/// A workspace as monitors and windows point at it.
#[derive(Debug, Default, Deserialize)]
#[serde(from = "RawWorkspaceRef")]
struct WorkspaceRef {
    /// 0 when it points at none (a monitor without a special workspace).
    id: WorkspaceId,
}

/// A monitor's special workspace: none shows when its address and name are
/// empty (older releases also write `id` 0).
type SpecialWorkspaceRef = WorkspaceRef;

#[derive(Debug, Deserialize)]
struct RawWorkspaceRef {
    id: Option<WorkspaceId>,
    address: Option<String>,
    #[serde(default)]
    name: String,
}

impl From<RawWorkspaceRef> for WorkspaceRef {
    fn from(raw: RawWorkspaceRef) -> Self {
        let address = raw.address.unwrap_or(raw.name);
        Self { id: raw.id.unwrap_or_else(|| address_id(&address)) }
    }
}

#[derive(Debug, Deserialize)]
#[serde(from = "RawWorkspace")]
struct Workspace {
    id: WorkspaceId,
    /// What dispatchers and events call it: `special:magic` for a special
    /// workspace, the number for a numbered one.
    address: String,
    /// What it shows as.
    name: String,
    monitor: String,
}

#[derive(Debug, Deserialize)]
struct RawWorkspace {
    id: Option<WorkspaceId>,
    address: Option<String>,
    name: String,
    monitor: String,
}

impl From<RawWorkspace> for Workspace {
    fn from(raw: RawWorkspace) -> Self {
        let address = raw.address.unwrap_or_else(|| raw.name.clone());
        Self { id: raw.id.unwrap_or_else(|| address_id(&address)), address, name: raw.name, monitor: raw.monitor }
    }
}

/// The id novad gives a workspace Hyprland gives none: 0.57 numbers only
/// numbered workspaces and calls special and named ones by address (older
/// releases gave those negative ids). A stable negative number from the
/// address, within the integers JavaScript keeps exactly, since the UI sends
/// ids back; 0 for no workspace.
fn address_id(address: &str) -> WorkspaceId {
    if address.is_empty() {
        return 0;
    }
    let hash = address.bytes().fold(FNV_OFFSET, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME));
    -WorkspaceId::try_from(hash >> (u64::BITS - JS_EXACT_BITS)).unwrap_or_default() - 1
}

#[derive(Debug, Deserialize)]
struct Client {
    address: String,
    class: String,
    /// Its class when it opened; some apps change theirs later.
    #[serde(rename = "initialClass", default)]
    initial_class: String,
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
    /// Each output's `layout`, by name: how often windows on it moved.
    moved: HashMap<String, u64>,
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
            moved: HashMap::new(),
        };
        state.catch_up(ask)?;
        Ok(state)
    }

    /// Applies one event line.
    pub(super) fn apply(&mut self, line: &str) {
        let Some((event, data)) = line.split_once(EVENT_SEPARATOR) else { return };
        match event {
            "workspacev2" | "focusedmonv2" | "createworkspacev2" | "destroyworkspacev2" | "moveworkspacev2"
            | "renameworkspace" | "monitoraddedv2" | "monitorremovedv2" | "configreloaded" | "activespecialv2" => {
                self.stale_workspaces = true;
            }
            "openwindow" => {
                self.stale_windows = true;
                // ADDRESS,WORKSPACENAME,CLASS,TITLE
                let workspace = data.split(',').nth(1).and_then(|name| self.workspace_named(name));
                self.moved_on(workspace);
            }
            "movewindowv2" => {
                self.stale_windows = true;
                // ADDRESS,WORKSPACE,NAME: where it was, and where it went.
                // WORKSPACE is the id before 0.57, its address since.
                let mut fields = data.split(',');
                let from = fields.next().and_then(|address| Some(self.windows.get(address)?.workspace));
                self.moved_on(from);
                let to = fields.next().and_then(|to| to.parse().ok().or_else(|| self.workspace_named(to)));
                self.moved_on(to);
            }
            "fullscreen" => {
                self.stale_windows = true;
                self.moved_on(None);
            }
            "changefloatingmode" => {
                let workspace = data.split(',').next().and_then(|address| Some(self.windows.get(address)?.workspace));
                self.moved_on(workspace);
            }
            "closewindow" => {
                let workspace = self.windows.remove(data).map(|window| window.workspace);
                self.urgent.remove(data);
                self.moved_on(workspace);
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

    fn workspace_named(&self, name: &str) -> Option<WorkspaceId> {
        self.workspaces.iter().find(|workspace| workspace.address == name).map(|workspace| workspace.id)
    }

    /// Windows moved on the monitor showing `workspace`, or the focused one
    /// when it isn't known (Hyprland reports no resizes).
    fn moved_on(&mut self, workspace: Option<WorkspaceId>) {
        let on = workspace.and_then(|id| self.workspaces.iter().find(|workspace| workspace.id == id));
        let monitor = match on {
            Some(workspace) => Some(workspace.monitor.clone()),
            None => self.monitors.iter().find(|monitor| monitor.focused).map(|monitor| monitor.name.clone()),
        };
        if let Some(monitor) = monitor {
            *self.moved.entry(monitor).or_default() += 1;
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
        // Special workspaces on every output, by name, as they show on
        // whichever has focus; then each output's own: numbered ones in
        // order, then named ones (negative ids).
        let mut special: Vec<(&Workspace, &str)> = self
            .workspaces
            .iter()
            .filter_map(|workspace| Some((workspace, workspace.address.strip_prefix(SPECIAL_PREFIX)?)))
            .collect();
        special.sort_by_key(|(_, name)| *name);
        let shown: HashSet<WorkspaceId> = self.monitors.iter().map(|monitor| monitor.special_workspace.id).collect();
        let outputs = self
            .monitors
            .iter()
            .map(|monitor| {
                let mut own: Vec<&Workspace> = self
                    .workspaces
                    .iter()
                    .filter(|workspace| {
                        workspace.monitor == monitor.name && !workspace.address.starts_with(SPECIAL_PREFIX)
                    })
                    .collect();
                own.sort_by_key(|workspace| (workspace.id < 0, workspace.id.unsigned_abs()));
                let covered = special.iter().any(|(workspace, _)| workspace.id == monitor.special_workspace.id);
                let listed = special
                    .iter()
                    .map(|&(workspace, name)| (workspace, name, true))
                    .chain(own.into_iter().map(|workspace| (workspace, workspace.name.as_str(), false)));
                Output {
                    name: monitor.name.clone(),
                    focused: monitor.focused,
                    layout: self.moved.get(&monitor.name).copied().unwrap_or_default(),
                    workspaces: listed
                        .map(|(workspace, name, special)| {
                            // A special workspace showing covers the one
                            // under it, which Hyprland still calls active.
                            let active = if special {
                                workspace.id == monitor.special_workspace.id
                            } else {
                                workspace.id == monitor.active_workspace.id && !covered
                            };
                            NovaWorkspace {
                                id: workspace.id,
                                name: name.to_owned(),
                                active,
                                focused: active && monitor.focused,
                                urgent: urgent_workspaces.contains(&workspace.id),
                                empty: !occupied.contains(&workspace.id),
                                fullscreen: fullscreen.contains(&workspace.id),
                                special,
                                shown_elsewhere: special && !active && shown.contains(&workspace.id),
                                active_window: None,
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
                .and_then(|address| self.windows.get(address).map(|window| (address, window)))
                .map(|(address, window)| FocusedWindow {
                    id: address.clone(),
                    app_id: window.app_id.clone(),
                    title: window.title.clone(),
                }),
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
