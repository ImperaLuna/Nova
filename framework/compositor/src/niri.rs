//! niri, through the socket in `$NIRI_SOCKET`. One connection asks for the
//! event stream and then reads it: niri first sends its whole state as
//! events, then each change. Actions are one request per connection. Both
//! are JSON lines in niri-ipc's format.
//!
//! niri says nothing about fullscreen, so a workspace counts as fullscreen
//! when a tiled window on it is as big as its output: niri gives fullscreen
//! (and maximized-to-edges) windows the whole output, over the bar's space.
//! Its width counts too: once the bar gives its space up, an ordinary
//! column can be as tall as the output (with no gaps), and taking that for
//! fullscreen would keep the bar away for good.
//! Output sizes aren't in the event stream, so novad asks for them again
//! after workspace or window layout changes (an output's size changing
//! resizes such windows too).
//!
//! Only the fields and events novad uses are read. Other events, and fields
//! newer niri releases add, are ignored, so those releases keep working.

use super::Backend;
use crate::{Capability, Desktop, FocusedWindow, Output, Workspace as NovaWorkspace, WorkspaceId};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub const SOCKET_VAR: &str = "NIRI_SOCKET";
const NAME: &str = "niri";
const CAPABILITIES: &[Capability] = &[
    Capability::Workspaces,
    Capability::FocusWorkspace,
    Capability::FocusedWindow,
    Capability::KeyboardLayout,
    Capability::LogOut,
    Capability::Fullscreen,
];
/// The request that turns a connection into the event stream.
const EVENT_STREAM_REQUEST: &str = r#""EventStream""#;
/// The request for every window.
const WINDOWS_REQUEST: &str = r#""Windows""#;
/// The request for every output, and the key its answer comes under.
const OUTPUTS_REQUEST: &str = r#""Outputs""#;
const OUTPUTS_ANSWER: &str = "Outputs";
/// How long niri may take to answer a request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Niri {
    socket: PathBuf,
}

impl Niri {
    pub fn new(socket: PathBuf) -> Self {
        Self { socket }
    }

    /// A window whose app id is one of `app_ids` (lower case), one on
    /// `here` first.
    fn app_window(&self, app_ids: &[String], here: &NovaWorkspace) -> Result<Option<Window>, String> {
        let here = u64::try_from(here.id).ok();
        let mut matching: Vec<Window> = self
            .windows()?
            .into_iter()
            .filter(|window| window.app_id.as_ref().is_some_and(|id| app_ids.contains(&id.to_lowercase())))
            .collect();
        matching.sort_by_key(|window| window.workspace_id != here);
        Ok(matching.into_iter().next())
    }

    /// Every window, as niri answers now.
    fn windows(&self) -> Result<Vec<Window>, String> {
        let reply = self.request(WINDOWS_REQUEST)?;
        serde_json::from_value(reply["Windows"].clone()).map_err(|e| format!("niri sent odd windows ({e})"))
    }

    fn connect(&self) -> Result<UnixStream, String> {
        UnixStream::connect(&self.socket).map_err(|e| format!("can't reach niri at {}: {e}", self.socket.display()))
    }

    /// Sends one request on its own connection and returns niri's answer.
    fn request(&self, request: &str) -> Result<serde_json::Value, String> {
        let mut stream = self.connect()?;
        stream.set_read_timeout(Some(REPLY_TIMEOUT)).map_err(|e| e.to_string())?;
        writeln!(stream, "{request}").map_err(|e| format!("can't send to niri: {e}"))?;
        read_reply(&mut BufReader::new(stream))
    }
}

impl Backend for Niri {
    fn name(&self) -> &'static str {
        NAME
    }

    fn capabilities(&self) -> &'static [Capability] {
        CAPABILITIES
    }

    fn watch(&self, report: &mut dyn FnMut(Desktop)) -> Result<(), String> {
        let mut stream = self.connect()?;
        // Only the answer to the request has a deadline; events come when they come.
        stream.set_read_timeout(Some(REPLY_TIMEOUT)).map_err(|e| e.to_string())?;
        writeln!(stream, "{EVENT_STREAM_REQUEST}").map_err(|e| format!("can't send to niri: {e}"))?;
        let mut reader = BufReader::new(stream);
        read_reply(&mut reader)?;
        reader.get_ref().set_read_timeout(None).map_err(|e| e.to_string())?;
        let mut state = State::default();
        let mut ask = || {
            let mut answer = self.request(OUTPUTS_REQUEST)?;
            answer
                .get_mut(OUTPUTS_ANSWER)
                .map(serde_json::Value::take)
                .ok_or_else(|| format!("niri sent an odd answer to {OUTPUTS_REQUEST}: {answer}"))
        };
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).map_err(|e| format!("can't read niri's events: {e}"))? == 0 {
                return Ok(());
            }
            state.apply(&line)?;
            // niri sends what one change causes together: report once it's all in.
            if reader.buffer().is_empty() {
                state.catch_up(&mut ask)?;
                report(state.desktop());
            }
        }
    }

    fn focus_workspace(&self, workspace: &NovaWorkspace) -> Result<(), String> {
        let id = u64::try_from(workspace.id).map_err(|_| format!("niri has no workspace {}", workspace.id))?;
        let action = json!({"Action": {"FocusWorkspace": {"reference": {"Id": id}}}});
        self.request(&action.to_string()).map(drop)
    }

    fn focus_output(&self, output: &Output) -> Result<(), String> {
        match output.workspaces.iter().find(|workspace| workspace.active) {
            Some(workspace) => self.focus_workspace(workspace),
            None => Ok(()),
        }
    }

    fn focus_window(&self, app_ids: &[String], here: &NovaWorkspace) -> Result<bool, String> {
        let Some(window) = self.app_window(app_ids, here)? else { return Ok(false) };
        // niri shows the window's workspace and output for it.
        let action = json!({"Action": {"FocusWindow": {"id": window.id}}});
        self.request(&action.to_string()).map(|_| true)
    }

    fn window_workspace(&self, app_ids: &[String], here: &NovaWorkspace) -> Result<Option<WorkspaceId>, String> {
        let window = self.app_window(app_ids, here)?;
        Ok(window.and_then(|window| WorkspaceId::try_from(window.workspace_id?).ok()))
    }

    fn log_out(&self) -> Result<(), String> {
        // The power menu already asked.
        let action = json!({"Action": {"Quit": {"skip_confirmation": true}}});
        self.request(&action.to_string()).map(drop)
    }
}

/// niri's answer to a request: `{"Ok": ...}` or `{"Err": "why"}`.
fn read_reply(reader: &mut impl BufRead) -> Result<serde_json::Value, String> {
    #[derive(Deserialize)]
    enum Reply {
        Ok(serde_json::Value),
        Err(String),
    }
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| format!("niri didn't answer: {e}"))?;
    match serde_json::from_str(&line).map_err(|e| format!("niri sent an odd reply ({e}): {}", line.trim()))? {
        Reply::Ok(value) => Ok(value),
        Reply::Err(message) => Err(format!("niri: {message}")),
    }
}

#[derive(Debug, Deserialize)]
enum Event {
    /// Replaces every workspace.
    WorkspacesChanged {
        workspaces: Vec<Workspace>,
    },
    /// `id` is now active on its output, and the focused one if `focused`.
    WorkspaceActivated {
        id: u64,
        focused: bool,
    },
    WorkspaceUrgencyChanged {
        id: u64,
        urgent: bool,
    },
    WorkspaceActiveWindowChanged {
        workspace_id: u64,
        active_window_id: Option<u64>,
    },
    /// Replaces every window.
    WindowsChanged {
        windows: Vec<Window>,
    },
    WindowOpenedOrChanged {
        window: Window,
    },
    WindowClosed {
        id: u64,
    },
    WindowFocusChanged {
        id: Option<u64>,
    },
    /// New layouts for some windows, as (id, layout) pairs.
    WindowLayoutsChanged {
        changes: Vec<(u64, Layout)>,
    },
    KeyboardLayoutsChanged {
        keyboard_layouts: KeyboardLayouts,
    },
    KeyboardLayoutSwitched {
        idx: usize,
    },
    /// Everything else niri sends, now or in later releases.
    #[serde(untagged)]
    Other(#[allow(dead_code)] serde_json::Value),
}

#[derive(Debug, Clone, Deserialize)]
struct Workspace {
    id: u64,
    /// Its position on its output, from 1.
    idx: u8,
    name: Option<String>,
    output: Option<String>,
    is_urgent: bool,
    is_active: bool,
    is_focused: bool,
    /// The window its view follows, focused or not.
    active_window_id: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct Window {
    id: u64,
    title: Option<String>,
    /// None while it's being opened, before niri places it.
    workspace_id: Option<u64>,
    app_id: Option<String>,
    is_focused: bool,
    /// Missing before niri 25.05.
    layout: Option<Layout>,
}

#[derive(Debug, Clone, Deserialize)]
struct Layout {
    /// None while it floats.
    pos_in_scrolling_layout: Option<(usize, usize)>,
    /// Width and height in logical pixels, borders included.
    tile_size: (f64, f64),
}

/// One output in niri's answer to `Outputs`.
#[derive(Debug, Deserialize)]
struct NiriOutput {
    /// None while it's off.
    logical: Option<LogicalSize>,
}

#[derive(Debug, Deserialize)]
struct LogicalSize {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct KeyboardLayouts {
    names: Vec<String>,
    current_idx: usize,
}

/// What the events so far add up to.
#[derive(Debug, Default)]
pub(super) struct State {
    workspaces: BTreeMap<u64, Workspace>,
    windows: HashMap<u64, Window>,
    focused_window: Option<u64>,
    layouts: Option<KeyboardLayouts>,
    /// Each output's width and height in logical pixels, by name.
    output_sizes: HashMap<String, (f64, f64)>,
    /// Whether `output_sizes` is up to date; asked for in `catch_up`.
    outputs_known: bool,
    /// Each output's `layout`, by name: how often windows on it moved.
    moved: HashMap<String, u64>,
}

impl State {
    /// Applies one line of the event stream.
    pub(super) fn apply(&mut self, line: &str) -> Result<(), String> {
        let event = serde_json::from_str(line).map_err(|e| format!("niri sent an odd event ({e}): {}", line.trim()))?;
        match event {
            Event::WorkspacesChanged { workspaces } => {
                self.workspaces = workspaces.into_iter().map(|workspace| (workspace.id, workspace)).collect();
                self.outputs_known = false;
            }
            Event::WorkspaceActivated { id, focused } => {
                let output = self.workspaces.get(&id).map(|workspace| workspace.output.clone());
                for workspace in self.workspaces.values_mut() {
                    if Some(&workspace.output) == output.as_ref() {
                        workspace.is_active = workspace.id == id;
                    }
                    if focused {
                        workspace.is_focused = workspace.id == id;
                    }
                }
            }
            Event::WorkspaceUrgencyChanged { id, urgent } => {
                if let Some(workspace) = self.workspaces.get_mut(&id) {
                    workspace.is_urgent = urgent;
                }
            }
            Event::WorkspaceActiveWindowChanged { workspace_id, active_window_id } => {
                if let Some(workspace) = self.workspaces.get_mut(&workspace_id) {
                    workspace.active_window_id = active_window_id;
                }
            }
            Event::WindowsChanged { windows } => {
                self.focused_window = windows.iter().find(|window| window.is_focused).map(|window| window.id);
                self.windows = windows.into_iter().map(|window| (window.id, window)).collect();
            }
            Event::WindowOpenedOrChanged { window } => {
                if window.is_focused {
                    self.focused_window = Some(window.id);
                } else if self.focused_window == Some(window.id) {
                    self.focused_window = None;
                }
                self.windows.insert(window.id, window);
            }
            Event::WindowClosed { id } => {
                self.windows.remove(&id);
                if self.focused_window == Some(id) {
                    self.focused_window = None;
                }
            }
            Event::WindowFocusChanged { id } => self.focused_window = id,
            Event::WindowLayoutsChanged { changes } => {
                for (id, layout) in changes {
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.layout = Some(layout);
                        let output = window.workspace_id.and_then(|id| self.workspaces.get(&id)?.output.clone());
                        if let Some(output) = output {
                            *self.moved.entry(output).or_default() += 1;
                        }
                    }
                }
                self.outputs_known = false;
            }
            Event::KeyboardLayoutsChanged { keyboard_layouts } => self.layouts = Some(keyboard_layouts),
            Event::KeyboardLayoutSwitched { idx } => {
                if let Some(layouts) = &mut self.layouts {
                    layouts.current_idx = idx;
                }
            }
            Event::Other(_) => {}
        }
        Ok(())
    }

    /// Asks for the output sizes when they may have changed: `ask` answers
    /// what niri answers `Outputs` with.
    pub(super) fn catch_up(
        &mut self,
        ask: &mut impl FnMut() -> Result<serde_json::Value, String>,
    ) -> Result<(), String> {
        if self.outputs_known {
            return Ok(());
        }
        let outputs: HashMap<String, NiriOutput> =
            serde_json::from_value(ask()?).map_err(|e| format!("niri sent odd outputs ({e})"))?;
        self.output_sizes = outputs
            .into_iter()
            .filter_map(|(name, output)| {
                let size = output.logical?;
                Some((name, (f64::from(size.width), f64::from(size.height))))
            })
            .collect();
        self.outputs_known = true;
        Ok(())
    }

    /// Whether a tiled window on `workspace` that may be on screen is as big
    /// as its output. niri doesn't say where its view is, only which column
    /// it follows (the active window's): a column as wide as the output
    /// shares the screen with that one only right beside it, so one further
    /// away is scrolled out of view (IMP-112).
    fn has_fullscreen(&self, workspace: &Workspace) -> bool {
        let Some((width, height)) = workspace.output.as_ref().and_then(|output| self.output_sizes.get(output)) else {
            return false;
        };
        let column = |window: &Window| window.layout.as_ref()?.pos_in_scrolling_layout.map(|(column, _)| column);
        let active_column = workspace.active_window_id.and_then(|id| self.windows.get(&id)).and_then(column);
        self.windows.values().any(|window| {
            window.workspace_id == Some(workspace.id)
                && window
                    .layout
                    .as_ref()
                    .is_some_and(|layout| layout.tile_size.0 >= *width && layout.tile_size.1 >= *height)
                && column(window).is_some_and(|at| active_column.is_none_or(|active| at.abs_diff(active) <= 1))
        })
    }

    pub(super) fn desktop(&self) -> Desktop {
        let occupied: HashSet<u64> = self.windows.values().filter_map(|window| window.workspace_id).collect();
        let mut outputs: BTreeMap<&str, Vec<&Workspace>> = BTreeMap::new();
        // A workspace without an output belongs to one that was unplugged;
        // niri moves it back when the output returns.
        for workspace in self.workspaces.values() {
            if let Some(output) = &workspace.output {
                outputs.entry(output).or_default().push(workspace);
            }
        }
        let outputs = outputs
            .into_iter()
            .map(|(name, mut workspaces)| {
                workspaces.sort_by_key(|workspace| workspace.idx);
                Output {
                    name: name.into(),
                    focused: workspaces.iter().any(|workspace| workspace.is_focused),
                    layout: self.moved.get(name).copied().unwrap_or_default(),
                    workspaces: workspaces
                        .into_iter()
                        .map(|workspace| NovaWorkspace {
                            // niri counts ids up from 1; one past i64 would
                            // take billions of years of new workspaces.
                            id: workspace.id.try_into().unwrap_or(WorkspaceId::MAX),
                            name: workspace.name.clone().unwrap_or_else(|| workspace.idx.to_string()),
                            active: workspace.is_active,
                            focused: workspace.is_focused,
                            urgent: workspace.is_urgent,
                            empty: !occupied.contains(&workspace.id),
                            fullscreen: self.has_fullscreen(workspace),
                            special: false,
                            shown_elsewhere: false,
                            active_window: workspace.active_window_id.map(|id| id.to_string()),
                        })
                        .collect(),
                }
            })
            .collect();
        Desktop {
            compositor: NAME.into(),
            capabilities: CAPABILITIES.to_vec(),
            outputs,
            focused_window: self.focused_window.and_then(|id| self.windows.get(&id)).map(|window| FocusedWindow {
                id: window.id.to_string(),
                app_id: window.app_id.clone().unwrap_or_default(),
                title: window.title.clone().unwrap_or_default(),
            }),
            keyboard_layout: self.layouts.as_ref().and_then(|layouts| layouts.names.get(layouts.current_idx).cloned()),
        }
    }
}
