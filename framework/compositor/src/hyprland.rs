use super::Backend;
use nova_desktop_info::{
    Capability, Desktop, FocusedWindow, HYPRLAND_LOCK_RESTORE, Output, Workspace as NovaWorkspace, WorkspaceId,
};
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
const EVENT_SEPARATOR: &str = ">>";
const SPECIAL_PREFIX: &str = "special:";
const ADDRESS_PREFIX: &str = "0x";
const FULLSCREEN_MODE: u8 = 2;
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const UI_ID_BITS: u32 = 31;
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

const MONITORS: &str = "j/monitors";
const WORKSPACES: &str = "j/workspaces";
const CLIENTS: &str = "j/clients";
const ACTIVE_WINDOW: &str = "j/activewindow";
const DEVICES: &str = "j/devices";
const GET_OPTION: &str = "j/getoption";

pub const LUA_PROBE: &str = "dispatch hl.dsp.no_op()";
const DISPATCHED: &str = "ok";

pub fn socket_dir(runtime_dir: &Path, instance: &std::ffi::OsStr) -> PathBuf {
    runtime_dir.join("hypr").join(instance)
}

pub struct Hyprland {
    dir: PathBuf,
    dialect: Mutex<Option<Dialect>>,
}

impl Hyprland {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, dialect: Mutex::new(None) }
    }

    fn request(&self, request: &str) -> Result<String, String> {
        let path = self.dir.join(REQUEST_SOCKET);
        let mut stream =
            UnixStream::connect(&path).map_err(|e| format!("can't reach Hyprland at {}: {e}", path.display()))?;
        stream.set_read_timeout(Some(REPLY_TIMEOUT)).map_err(|e| e.to_string())?;
        stream.write_all(request.as_bytes()).map_err(|e| format!("can't send to Hyprland: {e}"))?;
        // Hyprland reads a request until the client stops writing: https://wiki.hypr.land/IPC/
        let _ = stream.shutdown(Shutdown::Write);
        let mut answer = String::new();
        stream.read_to_string(&mut answer).map_err(|e| format!("Hyprland didn't answer {request}: {e}"))?;
        Ok(answer)
    }
}

impl Hyprland {
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

    fn clients(&self) -> Result<Vec<Client>, String> {
        let answer = self.request(CLIENTS)?;
        serde_json::from_str(&answer).map_err(|e| format!("Hyprland sent odd windows ({e})"))
    }

    fn dialect(&self) -> Result<Dialect, String> {
        let mut known = self.dialect.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(dialect) = *known {
            return Ok(dialect);
        }
        let dialect = if self.request(LUA_PROBE)?.trim() == DISPATCHED { Dialect::Lua } else { Dialect::Hyprlang };
        *known = Some(dialect);
        Ok(dialect)
    }

    fn dispatch(&self, action: &Action) -> Result<(), String> {
        let dispatcher = action.dispatcher(self.dialect()?);
        match self.request(&format!("dispatch {dispatcher}"))?.trim() {
            DISPATCHED => Ok(()),
            error => Err(format!("Hyprland: {error}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Hyprlang,
    Lua,
}

#[derive(Debug)]
enum Action {
    FocusWorkspace(String),
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
        self.dispatch(&Action::FocusMonitor(output.name.clone()))
    }

    fn focus_window(&self, app_ids: &[String], here: &NovaWorkspace) -> Result<bool, String> {
        let Some(client) = self.app_window(app_ids, here)? else { return Ok(false) };
        self.dispatch(&Action::FocusWindow(format!("address:{}", client.address)))?;
        Ok(true)
    }

    fn window_workspace(&self, app_ids: &[String], here: &NovaWorkspace) -> Result<Option<WorkspaceId>, String> {
        Ok(self.app_window(app_ids, here)?.map(|client| client.workspace.id))
    }

    fn log_out(&self) -> Result<(), String> {
        self.dispatch(&Action::Exit)
    }

    fn restores_session_lock(&self) -> Result<bool, String> {
        lock_restore_in(&self.request(&format!("{GET_OPTION} {HYPRLAND_LOCK_RESTORE}"))?)
    }
}

pub(super) fn lock_restore_in(answer: &str) -> Result<bool, String> {
    #[derive(Deserialize)]
    struct Switch {
        int: Option<i64>,
        #[serde(rename = "bool")]
        boolean: Option<bool>,
    }
    let odd = |problem: &dyn std::fmt::Display| {
        format!("Hyprland sent an odd answer about {HYPRLAND_LOCK_RESTORE} ({problem})")
    };
    let switch: Switch = serde_json::from_str(answer).map_err(|e| odd(&e))?;
    switch.boolean.or(switch.int.map(|value| value != 0)).ok_or_else(|| odd(&answer.trim()))
}

#[derive(Debug, Deserialize)]
struct Monitor {
    name: String,
    focused: bool,
    #[serde(rename = "activeWorkspace")]
    active_workspace: WorkspaceRef,
    #[serde(rename = "specialWorkspace", default)]
    special_workspace: SpecialWorkspaceRef,
}

#[derive(Debug, Default, Deserialize)]
#[serde(from = "RawWorkspaceRef")]
struct WorkspaceRef {
    id: WorkspaceId,
}

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
    address: String,
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

fn address_id(address: &str) -> WorkspaceId {
    if address.is_empty() {
        return 0;
    }
    let hash = address.bytes().fold(FNV_OFFSET, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME));
    -WorkspaceId::try_from(hash >> (u64::BITS - UI_ID_BITS)).unwrap_or_default() - 1
}

#[derive(Debug, Deserialize)]
struct Client {
    address: String,
    class: String,
    #[serde(rename = "initialClass", default)]
    initial_class: String,
    title: String,
    workspace: WorkspaceRef,
    fullscreen: u8,
}

#[derive(Debug, Deserialize)]
struct ActiveWindow {
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

#[derive(Debug)]
pub(super) struct State {
    monitors: Vec<Monitor>,
    workspaces: Vec<Workspace>,
    windows: HashMap<String, Window>,
    active_window: Option<String>,
    urgent: HashSet<String>,
    layout: Option<String>,
    stale_workspaces: bool,
    stale_windows: bool,
    stale_layout: bool,
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

    pub(super) fn apply(&mut self, line: &str) {
        let Some((event, data)) = line.split_once(EVENT_SEPARATOR) else { return };
        match event {
            "workspacev2" | "focusedmonv2" | "createworkspacev2" | "destroyworkspacev2" | "moveworkspacev2"
            | "renameworkspace" | "monitoraddedv2" | "monitorremovedv2" | "configreloaded" | "activespecialv2" => {
                self.stale_workspaces = true;
            }
            "changeworkspaceid" => {
                self.stale_workspaces = true;
                self.stale_windows = true;
            }
            "openwindow" => {
                self.stale_windows = true;
                let workspace = data.split(',').nth(1).and_then(|name| self.workspace_named(name));
                self.moved_on(workspace);
            }
            "movewindowv2" => {
                self.stale_windows = true;
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
            "activelayout" => self.stale_layout = true,
            _ => {}
        }
    }

    fn workspace_named(&self, name: &str) -> Option<WorkspaceId> {
        self.workspaces.iter().find(|workspace| workspace.address == name).map(|workspace| workspace.id)
    }

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
