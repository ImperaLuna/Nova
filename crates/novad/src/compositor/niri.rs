//! niri, through the socket in `$NIRI_SOCKET`. One connection asks for the
//! event stream and then reads it: niri first sends its whole state as
//! events, then each change. Actions are one request per connection. Both
//! are JSON lines in niri-ipc's format.
//!
//! Only the fields and events novad uses are read. Other events, and fields
//! newer niri releases add, are ignored, so those releases keep working.

use super::Backend;
use nova_protocol::{Capability, Desktop, FocusedWindow, Output, Workspace as NovaWorkspace, WorkspaceId};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub const SOCKET_VAR: &str = "NIRI_SOCKET";
const NAME: &str = "niri";
const CAPABILITIES: &[Capability] =
    &[Capability::Workspaces, Capability::FocusWorkspace, Capability::FocusedWindow, Capability::KeyboardLayout];
/// The request that turns a connection into the event stream.
const EVENT_STREAM_REQUEST: &str = r#""EventStream""#;
/// How long niri may take to answer a request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Niri {
    socket: PathBuf,
}

impl Niri {
    pub fn new(socket: PathBuf) -> Self {
        Self { socket }
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
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).map_err(|e| format!("can't read niri's events: {e}"))? == 0 {
                return Ok(());
            }
            state.apply(&line)?;
            // niri sends what one change causes together: report once it's all in.
            if reader.buffer().is_empty() {
                report(state.desktop());
            }
        }
    }

    fn focus_workspace(&self, workspace: &NovaWorkspace) -> Result<(), String> {
        let id = u64::try_from(workspace.id).map_err(|_| format!("niri has no workspace {}", workspace.id))?;
        let action = json!({"Action": {"FocusWorkspace": {"reference": {"Id": id}}}});
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
}

#[derive(Debug, Clone, Deserialize)]
struct Window {
    id: u64,
    title: Option<String>,
    app_id: Option<String>,
    is_focused: bool,
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
}

impl State {
    /// Applies one line of the event stream.
    pub(super) fn apply(&mut self, line: &str) -> Result<(), String> {
        let event = serde_json::from_str(line).map_err(|e| format!("niri sent an odd event ({e}): {}", line.trim()))?;
        match event {
            Event::WorkspacesChanged { workspaces } => {
                self.workspaces = workspaces.into_iter().map(|workspace| (workspace.id, workspace)).collect();
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

    pub(super) fn desktop(&self) -> Desktop {
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
                app_id: window.app_id.clone().unwrap_or_default(),
                title: window.title.clone().unwrap_or_default(),
            }),
            keyboard_layout: self.layouts.as_ref().and_then(|layouts| layouts.names.get(layouts.current_idx).cloned()),
        }
    }
}
