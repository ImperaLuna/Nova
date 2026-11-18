//! The backends against what real niri and Hyprland sent, recorded in a VM
//! by `e2e/vm/record.py` (`just record`): `recorded/<compositor>/events.txt`
//! is the event stream, `steps.json` says how far into it each step of the
//! script had got, and `<step>/` holds the compositor's answers to its
//! queries at that point. `recorded/hyprland-<version>/` holds one set of
//! answers from a newer Hyprland, taken on a real desktop.

use super::*;
use nova_protocol::{FocusedWindow, Output};
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

/// niri's answer to `Outputs` at the end of step `index`.
fn niri_ask(recording: &Recording, index: usize) -> impl FnMut() -> Result<serde_json::Value, String> + '_ {
    move || Ok(serde_json::from_str(&recording.answer(index, "outputs")).unwrap())
}

fn follow_niri(recording: &Recording) -> Followed {
    let mut state = niri::State::default();
    let mut followed = Vec::new();
    for (index, step) in recording.steps.iter().enumerate() {
        // niri's answer to the event stream request comes first.
        for line in recording.lines(index).filter(|line| *line != NIRI_HANDLED) {
            state.apply(line).unwrap();
        }
        state.catch_up(&mut niri_ask(recording, index)).unwrap();
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
    state.catch_up(&mut niri_ask(recording, index)).unwrap();
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

/// Without the counts of windows moving: only events count them, so an
/// answer to a query always says 0.
fn without_layout(mut desktop: Desktop) -> Desktop {
    for output in &mut desktop.outputs {
        output.layout = 0;
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
    let first = window(step(followed, "open first window")).expect("the first window is focused");
    assert_eq!((first.app_id.as_str(), first.title.as_str()), ("nova-first", "First window"));
    assert!(!first.id.is_empty());
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
    assert_ne!(window(renamed).unwrap().id, first.id, "another window, told apart by its id");

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

    let fullscreen = |desktop: &Desktop| -> Vec<String> {
        let workspaces = desktop.outputs.iter().flat_map(|output| &output.workspaces);
        workspaces.filter(|w| w.fullscreen).map(|w| w.name.clone()).collect()
    };
    let before = &followed[..followed.iter().position(|(name, _)| name == "fullscreen the focused window").unwrap()];
    assert!(before.iter().all(|(_, desktop)| fullscreen(desktop).is_empty()), "fullscreen too early: {before:#?}");
    let full = step(followed, "fullscreen the focused window");
    assert_eq!(fullscreen(full), [focused_workspace(full).name.clone()], "{full:#?}");
    // Its window grew: the screen's layout count goes up, for glass to copy
    // the screen again.
    let layout = |desktop: &Desktop| desktop.outputs.iter().find(|output| output.focused).map(|output| output.layout);
    assert!(layout(full) > layout(&before.last().unwrap().1), "{full:#?}");

    let closed = step(followed, "close the focused window");
    assert_eq!(window(closed), None);
    assert!(fullscreen(closed).is_empty(), "still fullscreen once it closed: {closed:#?}");
}

fn special(desktop: &Desktop) -> Vec<&Workspace> {
    desktop.outputs.iter().flat_map(|output| &output.workspaces).filter(|w| w.special).collect()
}

/// Hyprland's special workspace, from the end of the recorded script.
fn check_the_special_workspace(followed: &Followed) {
    let before = step(followed, "close the focused window");
    assert!(special(before).is_empty(), "no special workspace yet: {before:#?}");

    let hidden = step(followed, "hide a window in the special workspace");
    let output = &hidden.outputs[0];
    let first = &output.workspaces[0];
    assert!(first.special, "special workspaces come first: {hidden:#?}");
    assert_eq!(first.name, "magic", "without the special: prefix");
    assert!(!first.empty && !first.active && !first.focused, "{first:#?}");
    assert_eq!(special(hidden).len(), 1);

    let shown = step(followed, "show the special workspace");
    assert_eq!(focused_workspace(shown).name, "magic", "the keyboard is on it");
    let active: Vec<&str> = shown.outputs[0].workspaces.iter().filter(|w| w.active).map(|w| w.name.as_str()).collect();
    assert_eq!(active, ["magic"], "the workspace under it isn't active: {shown:#?}");
    assert!(!special(shown)[0].shown_elsewhere, "it shows here");
    assert_eq!(window(shown).map(|w| w.app_id), Some("nova-second".into()));

    let gone = step(followed, "hide the special workspace");
    assert_eq!(focused_workspace(gone).name, "1", "back on the workspace under it");
    let magic = special(gone);
    assert_eq!(magic.len(), 1, "still there with its window: {gone:#?}");
    assert!(!magic[0].active && !magic[0].focused);
    assert!(!focused_workspace(gone).special);
}

fn window(desktop: &Desktop) -> Option<FocusedWindow> {
    desktop.focused_window.clone()
}

#[test]
fn niri_events_add_up_to_what_niri_reports() {
    let recording = Recording::load("niri");
    let followed = follow_niri(&recording);
    for (index, (name, desktop)) in followed.iter().enumerate() {
        assert_eq!(without_layout(desktop.clone()), niri_answer(&recording, index), "after {name:?}");
    }
    check_the_script(&followed);
    // A Nova panel holding the keyboard leaves no window focused; the
    // workspace still says which one focus goes back to.
    let opened = step(&followed, "open first window");
    assert_eq!(focused_workspace(opened).active_window, window(opened).map(|window| window.id));
}

#[test]
fn hyprland_events_add_up_to_what_hyprland_reports() {
    let recording = Recording::load("hyprland");
    let followed = follow_hyprland(&recording);
    for (index, (name, desktop)) in followed.iter().enumerate() {
        assert_eq!(
            without_urgency(without_layout(desktop.clone())),
            hyprland_answer(&recording, index),
            "after {name:?}"
        );
    }
    check_the_script(&followed);
    check_the_special_workspace(&followed);
}

#[test]
fn hyprland_0_57_without_a_special_workspace_id_still_loads() {
    // Answers from a real Hyprland 0.57 desktop with two outputs, kept in
    // `recorded/hyprland-0.57/`: its monitors leave out the special
    // workspace's `id` when none shows.
    let desktop = hyprland_0_57("hyprland-0.57").desktop();
    let names: Vec<&str> = desktop.outputs.iter().map(|output| output.name.as_str()).collect();
    assert_eq!(names, ["DP-1", "DP-2"]);
    assert!(special(&desktop).is_empty(), "{desktop:#?}");
    assert_eq!(focused_workspace(&desktop).name, "1");
}

/// Hyprland 0.57 as it answers from `recorded/<dir>/`.
fn hyprland_0_57(dir: &str) -> hyprland::State {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/compositor/recorded").join(dir);
    let mut ask = |request: &str| {
        let query = request.strip_prefix("j/").unwrap();
        Ok(std::fs::read_to_string(dir.join(format!("{query}.json"))).unwrap())
    };
    hyprland::State::load(&mut ask).unwrap()
}

#[test]
fn hyprland_0_57_shows_its_special_workspace() {
    // Answers from the same desktop on Hyprland 0.57, kept in
    // `recorded/hyprland-0.57-special/`: `special:magic` shows on DP-1 with a
    // fullscreen window, and DP-2 has focus. Special workspaces and their
    // windows have no `id` there, only an address.
    let desktop = hyprland_0_57("hyprland-0.57-special").desktop();
    let [here, there] = &desktop.outputs[..] else { panic!("{desktop:#?}") };

    let magic = &here.workspaces[0];
    assert!(magic.special && magic.active && !magic.focused, "{here:#?}");
    assert_eq!(magic.name, "magic");
    assert!(!magic.empty && magic.fullscreen, "its fullscreen window is in it");
    assert!(magic.id < 0, "an id of its own: {}", magic.id);
    assert!(i32::try_from(magic.id).is_ok(), "one a QML int keeps whole: {}", magic.id);
    assert!(!here.workspaces[1].active, "covered by the special workspace");

    let elsewhere = &there.workspaces[0];
    assert_eq!(elsewhere.id, magic.id, "the same workspace on every output");
    assert!(elsewhere.shown_elsewhere && !elsewhere.active);
    assert_eq!(focused_workspace(&desktop).name, "2");
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

#[test]
fn stepping_skips_the_special_workspace() {
    let followed = follow_hyprland(&Recording::load("hyprland"));
    let desktop = step(&followed, "show the special workspace");
    let output = &desktop.outputs[0].name;
    // From the workspace under it, never onto it.
    assert_eq!(neighbour(desktop, output, Step::Previous).unwrap(), None, "{desktop:#?}");
    let next = neighbour(desktop, output, Step::Next).unwrap();
    assert!(next.is_none_or(|workspace| !workspace.special));
    // Hidden, it comes first in the list; stepping back from 1 stays there.
    let hidden = step(&followed, "hide a window in the special workspace");
    assert!(hidden.outputs[0].workspaces[0].special, "{hidden:#?}");
    assert_eq!(neighbour(hidden, output, Step::Previous).unwrap(), None);
}

#[test]
fn a_column_as_tall_as_the_output_is_not_fullscreen() {
    // With no gaps, once the bar gives its space up an ordinary column is
    // as tall as the output; only one as wide too is fullscreen.
    let recording = Recording::load("niri");
    let fullscreen_step = recording.steps.iter().position(|step| step.name == "fullscreen the focused window").unwrap();
    let mut state = niri::State::default();
    for index in 0..=fullscreen_step {
        for line in recording.lines(index).filter(|line| *line != NIRI_HANDLED) {
            state.apply(line).unwrap();
        }
    }
    state.catch_up(&mut niri_ask(&recording, fullscreen_step)).unwrap();
    let shown = |desktop: &Desktop| focused_workspace(desktop).fullscreen;
    assert!(shown(&state.desktop()));

    let (width, height) = (1280.0 / 2.0, 800.0);
    let layout = serde_json::json!({"pos_in_scrolling_layout": [1, 1], "tile_size": [width, height]});
    let window = focused_window_id(&recording, fullscreen_step);
    state.apply(&serde_json::json!({"WindowLayoutsChanged": {"changes": [[window, layout]]}}).to_string()).unwrap();
    assert!(!shown(&state.desktop()));
}

/// The id of the window niri lists as focused at the end of step `index`.
fn focused_window_id(recording: &Recording, index: usize) -> u64 {
    let window: serde_json::Value = serde_json::from_str(&recording.answer(index, "focused-window")).unwrap();
    window["id"].as_u64().unwrap()
}

#[test]
fn a_fullscreen_column_scrolled_out_of_view_is_not_fullscreen() {
    // RyzenShine (IMP-112): a fullscreen browser two columns right of the
    // focused window, out of view, hid the bar.
    let recording = Recording::load("niri");
    let fullscreen_step = recording.steps.iter().position(|step| step.name == "fullscreen the focused window").unwrap();
    let mut state = niri::State::default();
    for index in 0..=fullscreen_step {
        for line in recording.lines(index).filter(|line| *line != NIRI_HANDLED) {
            state.apply(line).unwrap();
        }
    }
    state.catch_up(&mut niri_ask(&recording, fullscreen_step)).unwrap();
    let shown = |state: &niri::State| focused_workspace(&state.desktop()).fullscreen;
    assert!(shown(&state));

    let full = focused_window_id(&recording, fullscreen_step);
    let workspace = focused_workspace(&state.desktop()).id;
    let (width, height) = (1280.0, 800.0);
    // Another window in column 1 becomes the one the view follows, then the
    // fullscreen one moves to column 2 (beside it, maybe on screen) and 3.
    let other = full + 100;
    let window = |id: u64, column: usize, tile: (f64, f64)| {
        serde_json::json!({"WindowOpenedOrChanged": {"window": {
            "id": id, "title": "x", "workspace_id": workspace, "app_id": "x", "is_focused": id == other,
            "layout": {"pos_in_scrolling_layout": [column, 1], "tile_size": [tile.0, tile.1]},
        }}})
        .to_string()
    };
    state.apply(&window(other, 1, (width / 2.0, height))).unwrap();
    let active =
        serde_json::json!({"WorkspaceActiveWindowChanged": {"workspace_id": workspace, "active_window_id": other}});
    state.apply(&active.to_string()).unwrap();
    state.apply(&window(full, 2, (width, height))).unwrap();
    assert!(shown(&state), "beside the active column, it can share the screen");
    state.apply(&window(full, 3, (width, height))).unwrap();
    assert!(!shown(&state), "two columns away, it's out of view");
}

/// A compositor following `backend`, last reported `desktop`.
fn compositor(backend: Box<dyn Backend>, desktop: Option<Desktop>) -> Compositor {
    Compositor { backend, desktop: Mutex::new(desktop), hub: Arc::default(), output_listeners: Mutex::default() }
}

/// Remembers the workspaces it was asked to show.
#[derive(Default)]
struct Shown(Mutex<Vec<WorkspaceId>>);

impl Backend for Arc<Shown> {
    fn name(&self) -> &'static str {
        "shown"
    }

    fn capabilities(&self) -> &'static [Capability] {
        &[Capability::FocusWorkspace]
    }

    fn watch(&self, _: &mut dyn FnMut(Desktop)) -> Result<(), String> {
        Ok(())
    }

    fn focus_workspace(&self, workspace: &Workspace) -> Result<(), String> {
        lock(&self.0).push(workspace.id);
        Ok(())
    }

    fn focus_output(&self, output: &Output) -> Result<(), String> {
        let active = output.workspaces.iter().find(|workspace| workspace.active).ok_or("none shows")?;
        self.focus_workspace(active)
    }

    fn log_out(&self) -> Result<(), String> {
        Ok(())
    }

    fn focus_window(&self, _: &[String], _: &Workspace) -> Result<bool, String> {
        Ok(false)
    }

    fn window_workspace(&self, _: &[String], _: &Workspace) -> Result<Option<WorkspaceId>, String> {
        Ok(None)
    }
}

#[test]
fn focusing_an_output_shows_its_workspace_unless_it_has_focus() {
    // The recorded desktop, plus a second output without focus.
    let mut desktop = follow_niri(&Recording::load("niri")).pop().unwrap().1;
    let mut other = desktop.outputs[0].clone();
    other.name = "DP-2".into();
    other.focused = false;
    for workspace in &mut other.workspaces {
        workspace.id += 100;
    }
    let active = other.workspaces.iter().find(|workspace| workspace.active).unwrap().id;
    let focused = desktop.outputs[0].name.clone();
    desktop.outputs.push(other);
    let shown = Arc::new(Shown::default());
    let compositor = compositor(Box::new(Arc::clone(&shown)), Some(desktop));

    compositor.focus_output(&focused).unwrap();
    assert!(lock(&shown.0).is_empty(), "the focused output stays as it is");
    compositor.focus_output("DP-2").unwrap();
    assert_eq!(*lock(&shown.0), [active]);
    assert!(compositor.focus_output("nope").unwrap_err().contains("no such output"));
}

#[test]
fn a_special_workspace_shows_on_one_output_and_is_listed_on_every_one() {
    // The recording's single output, plus a second one without focus or
    // workspaces of its own, at the step that shows the special workspace.
    let recording = Recording::load("hyprland");
    let index = recording.steps.iter().position(|step| step.name == "show the special workspace").unwrap();
    let mut ask = |request: &str| {
        let answer = hyprland_ask(&recording, index)(request)?;
        if request != "j/monitors" {
            return Ok(answer);
        }
        let mut monitors: Vec<serde_json::Value> = serde_json::from_str(&answer).unwrap();
        let mut other = monitors[0].clone();
        other["name"] = "DP-2".into();
        other["focused"] = false.into();
        other["specialWorkspace"] = serde_json::json!({"id": 0, "name": ""});
        monitors.push(other);
        Ok(serde_json::to_string(&monitors).unwrap())
    };
    let desktop = hyprland::State::load(&mut ask).unwrap().desktop();
    let [here, there] = &desktop.outputs[..] else { panic!("two outputs: {desktop:#?}") };
    let magic = |output: &Output| output.workspaces[0].clone();
    assert!(magic(here).special && magic(there).special, "listed first on both");
    assert!(magic(here).active && magic(here).focused && !magic(here).shown_elsewhere);
    assert!(!magic(there).active && magic(there).shown_elsewhere, "{there:#?}");
    let active = |output: &Output| output.workspaces.iter().filter(|w| w.active).count();
    assert_eq!(active(here), 1, "only the special workspace: {here:#?}");
    assert_eq!(there.workspaces.len(), 1, "the recording's workspaces are all on the first output");
}

#[test]
fn output_listeners_hear_only_of_output_changes() {
    let desktop = follow_niri(&Recording::load("niri")).pop().unwrap().1;
    let compositor = compositor(Box::new(Arc::new(Shown::default())), None);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let hearing = Arc::clone(&heard);
    compositor.on_outputs(move |names| lock(&hearing).push(names));
    assert!(lock(&heard).is_empty(), "nothing before the first report");

    compositor.update(desktop.clone());
    let first: Vec<String> = desktop.outputs.iter().map(|output| output.name.clone()).collect();
    assert_eq!(*lock(&heard), [first]);
    // A workspace change alone isn't an output change.
    let mut moved = desktop.clone();
    moved.keyboard_layout = Some("Romanian".into());
    compositor.update(moved);
    assert_eq!(lock(&heard).len(), 1);
    // A second output is.
    let mut plugged = desktop.clone();
    let mut other = plugged.outputs[0].clone();
    other.name = "ZZ-1".into();
    plugged.outputs.push(other);
    compositor.update(plugged);
    assert_eq!(lock(&heard).last().unwrap().last().unwrap(), "ZZ-1");
    // One that listens later hears of the outputs at once.
    let late = Arc::new(Mutex::new(Vec::new()));
    let hearing = Arc::clone(&late);
    compositor.on_outputs(move |names| lock(&hearing).push(names));
    assert_eq!(lock(&late).len(), 1);
}
