//! The backends against what real niri and Hyprland sent, recorded in a VM
//! by `e2e/vm/record.py` (`just record`): `recorded/<compositor>/events.txt`
//! is the event stream, `steps.json` says how far into it each step of the
//! script had got, and `<step>/` holds the compositor's answers to its
//! queries at that point.

use super::*;
use nova_protocol::FocusedWindow;
use std::path::Path;

const NIRI_HANDLED: &str = r#"{"Ok":"Handled"}"#;

struct Recording {
    dir: PathBuf,
    events: String,
    steps: Vec<RecordedStep>,
}

#[derive(serde::Deserialize)]
struct RecordedStep {
    name: String,
    events_bytes: usize,
}

impl Recording {
    fn load(compositor: &str) -> Self {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/compositor/recorded").join(compositor);
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
        Self { events: read("events.txt"), steps: serde_json::from_str(&read("steps.json")).unwrap(), dir }
    }

    /// The event lines that arrived during step `index`.
    fn lines(&self, index: usize) -> impl Iterator<Item = &str> {
        let start = index.checked_sub(1).map_or(0, |previous| self.steps[previous].events_bytes);
        self.events[start..self.steps[index].events_bytes].lines()
    }

    /// The compositor's answer to `query` at the end of step `index`.
    fn answer(&self, index: usize, query: &str) -> String {
        std::fs::read_to_string(self.dir.join(format!("{index:02}")).join(format!("{query}.json"))).unwrap()
    }
}

/// The desktop after each step, from the backend following the events.
type Followed = Vec<(String, Desktop)>;

fn follow_niri(recording: &Recording) -> Followed {
    let mut state = niri::State::default();
    let mut followed = Vec::new();
    for (index, step) in recording.steps.iter().enumerate() {
        // niri's answer to the event stream request comes first.
        for line in recording.lines(index).filter(|line| *line != NIRI_HANDLED) {
            state.apply(line).unwrap();
        }
        followed.push((step.name.clone(), state.desktop()));
    }
    followed
}

/// What niri itself reports at step `index`, through the same model.
fn niri_answer(recording: &Recording, index: usize) -> Desktop {
    let mut state = niri::State::default();
    let event = |name: &str, field: &str, query: &str| {
        let answer: serde_json::Value = serde_json::from_str(&recording.answer(index, query)).unwrap();
        serde_json::json!({ name: { field: answer } }).to_string()
    };
    state.apply(&event("WorkspacesChanged", "workspaces", "workspaces")).unwrap();
    state.apply(&event("WindowsChanged", "windows", "windows")).unwrap();
    state.apply(&event("KeyboardLayoutsChanged", "keyboard_layouts", "keyboard-layouts")).unwrap();
    state.desktop()
}

fn hyprland_ask(recording: &Recording, index: usize) -> impl FnMut(&str) -> Result<String, String> + '_ {
    move |request: &str| {
        let query = request.strip_prefix("j/").unwrap();
        Ok(recording.answer(index, query))
    }
}

fn follow_hyprland(recording: &Recording) -> Followed {
    let mut state = hyprland::State::load(&mut hyprland_ask(recording, 0)).unwrap();
    let mut followed = Vec::new();
    for (index, step) in recording.steps.iter().enumerate() {
        for line in recording.lines(index) {
            state.apply(line);
        }
        // What the events asked novad to look up, answered as Hyprland did then.
        state.catch_up(&mut hyprland_ask(recording, index)).unwrap();
        followed.push((step.name.clone(), state.desktop()));
    }
    followed
}

/// What Hyprland itself reports at step `index`. Urgency is only ever in
/// events, so this has none.
fn hyprland_answer(recording: &Recording, index: usize) -> Desktop {
    hyprland::State::load(&mut hyprland_ask(recording, index)).unwrap().desktop()
}

fn without_urgency(mut desktop: Desktop) -> Desktop {
    for workspace in desktop.outputs.iter_mut().flat_map(|output| output.workspaces.iter_mut()) {
        workspace.urgent = false;
    }
    desktop
}

fn focused_workspace(desktop: &Desktop) -> &Workspace {
    let mut focused = desktop.outputs.iter().flat_map(|output| &output.workspaces).filter(|w| w.focused);
    let workspace = focused.next().expect("a focused workspace");
    assert!(focused.next().is_none(), "two focused workspaces: {desktop:#?}");
    workspace
}

fn step<'a>(followed: &'a Followed, name: &str) -> &'a Desktop {
    &followed.iter().find(|(step, _)| step == name).unwrap_or_else(|| panic!("no step {name:?}")).1
}

/// What the recorded script did, as the bar should see it, on any compositor.
fn check_the_script(followed: &Followed) {
    let window = |desktop: &Desktop| desktop.focused_window.clone();
    let first = FocusedWindow { app_id: "nova-first".into(), title: "First window".into() };

    assert_eq!(window(step(followed, "open first window")), Some(first.clone()));
    assert_eq!(focused_workspace(step(followed, "open first window")).name, "1");
    let on_two = step(followed, "focus the next workspace");
    assert_eq!(focused_workspace(on_two).name, "2");
    assert_eq!(window(on_two), None, "workspace 2 is empty");
    assert!(focused_workspace(on_two).empty);
    let first_workspace = on_two.outputs.iter().flat_map(|output| &output.workspaces).find(|w| w.name == "1");
    assert!(!first_workspace.expect("workspace 1").empty, "the first window is on workspace 1");

    let renamed = step(followed, "retitle the second window");
    assert_eq!(window(renamed).unwrap().title, "Renamed, with, commas");
    assert_eq!(window(renamed).unwrap().app_id, "nova-second");

    let before = step(followed, "retitle the second window").keyboard_layout.clone();
    let after = step(followed, "switch the keyboard layout").keyboard_layout.clone();
    assert_eq!(before.as_deref(), Some("English (US)"));
    assert_eq!(after.as_deref(), Some("German"));

    assert_eq!(focused_workspace(step(followed, "name the workspace")).name, "web");

    let back = step(followed, "focus the previous workspace");
    assert_eq!(focused_workspace(back).name, "1");
    assert_eq!(window(back), Some(first));
    assert!(back.outputs.iter().flat_map(|output| &output.workspaces).all(|workspace| !workspace.urgent));

    let rung = step(followed, "ring the second window's bell");
    let urgent: Vec<&str> = rung
        .outputs
        .iter()
        .flat_map(|output| &output.workspaces)
        .filter(|w| w.urgent)
        .map(|w| w.name.as_str())
        .collect();
    assert_eq!(urgent, ["web"], "{rung:#?}");

    let closed = followed.last().unwrap();
    assert_eq!(closed.0, "close the focused window");
    assert_eq!(window(&closed.1), None);
}

#[test]
fn niri_events_add_up_to_what_niri_reports() {
    let recording = Recording::load("niri");
    let followed = follow_niri(&recording);
    for (index, (name, desktop)) in followed.iter().enumerate() {
        assert_eq!(desktop, &niri_answer(&recording, index), "after {name:?}");
    }
    check_the_script(&followed);
}

#[test]
fn hyprland_events_add_up_to_what_hyprland_reports() {
    let recording = Recording::load("hyprland");
    let followed = follow_hyprland(&recording);
    for (index, (name, desktop)) in followed.iter().enumerate() {
        assert_eq!(without_urgency(desktop.clone()), hyprland_answer(&recording, index), "after {name:?}");
    }
    check_the_script(&followed);
}

#[test]
fn the_compositor_comes_from_its_variables() {
    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        move |name| vars.iter().find(|(var, _)| *var == name).map(|(_, value)| OsString::from(value))
    }
    assert_eq!(detect(env(&[("NIRI_SOCKET", "/run/user/1000/niri.sock")])).unwrap().name(), "niri");
    let hyprland = env(&[("HYPRLAND_INSTANCE_SIGNATURE", "abc"), ("XDG_RUNTIME_DIR", "/run/user/1000")]);
    assert_eq!(detect(hyprland).unwrap().name(), "Hyprland");
    assert!(detect(env(&[("NIRI_SOCKET", "")])).is_none());
    assert!(detect(env(&[("SWAYSOCK", "/run/user/1000/sway.sock")])).is_none());
}

#[test]
fn stepping_stops_at_either_end() {
    let followed = follow_niri(&Recording::load("niri"));
    let desktop = step(&followed, "focus the previous workspace");
    let output = &desktop.outputs.iter().find(|output| output.focused).unwrap().name;
    assert_eq!(neighbour(desktop, output, Step::Next).unwrap().unwrap().name, "web");
    assert_eq!(neighbour(desktop, output, Step::Previous).unwrap(), None);
    assert!(neighbour(desktop, "nope", Step::Next).unwrap_err().contains("no output"));
}
