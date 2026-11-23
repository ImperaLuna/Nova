//! The numbers as they are drawn: which workspaces show, where each one
//! sits, and what of it is on its way somewhere. Plain values over time,
//! tested alone; `list.rs` draws them.
//!
//! Numbers are kept by workspace id, so one that stays glides to its new
//! place when others come and go, and only a new one grows in.

use nova_desktop_info::{Capability, Desktop, Workspace, WorkspaceId};
use nova_look::motion::Eased;
use nova_look::tokens::{
    DURATION_COLOR, DURATION_FADE, DURATION_PANEL, ENTER_SCALE, WORKSPACE_GAP, WORKSPACE_GROUP_GAP, WORKSPACE_SIZE,
};
use std::time::{Duration, Instant};

/// The workspaces of `output` the bar lists: those with windows, the one
/// it shows, and a special one showing elsewhere. None when the compositor
/// lists no workspaces (`Protocol.barWorkspaces`).
pub fn shown(desktop: &Desktop, output: &str) -> Vec<Workspace> {
    if !desktop.capabilities.contains(&Capability::Workspaces) {
        return Vec::new();
    }
    let Some(output) = desktop.outputs.iter().find(|each| each.name == output) else { return Vec::new() };
    output
        .workspaces
        .iter()
        .filter(|workspace| !workspace.empty || workspace.active || workspace.shown_elsewhere)
        .cloned()
        .collect()
}

/// A number's color, by what it means; the theme gives the color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// On the accent capsule.
    OnAccent,
    /// Its window wants attention.
    Calling,
    /// A special workspace that isn't showing anywhere.
    Dim,
    Plain,
}

/// A value that changes over `duration` from what it was to what it is,
/// like a color animation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fade<T> {
    from: T,
    to: T,
    progress: Eased,
    duration: Duration,
}

impl<T: Copy + PartialEq> Fade<T> {
    pub fn new(value: T, duration: Duration, now: Instant) -> Self {
        Self { from: value, to: value, progress: Eased::lasting(1.0, duration, now), duration }
    }

    /// Heads for `value`, from the one it was heading for.
    pub fn set(&mut self, value: T, now: Instant) {
        if value == self.to {
            return;
        }
        self.from = self.to;
        self.to = value;
        let mut progress = Eased::lasting(0.0, self.duration, now);
        progress.set_target(1.0, now);
        self.progress = progress;
    }

    /// Where it was, where it goes, and how far it has come (0..1).
    pub fn at(&self, now: Instant) -> (T, T, f32) {
        (self.from, self.to, self.progress.value(now))
    }

    pub fn settled(&self, now: Instant) -> bool {
        self.progress.settled(now)
    }
}

/// One workspace's number.
#[derive(Debug, Clone, PartialEq)]
pub struct Number {
    pub workspace: Workspace,
    /// From the list's left edge.
    pub x: Eased,
    /// Its size, growing in from `ENTER_SCALE` when it is new.
    pub scale: Eased,
    /// Fading in when it is new.
    pub opacity: Eased,
    pub ink: Fade<Ink>,
    /// The "wants attention" dot's opacity.
    pub dot: Eased,
}

impl Number {
    fn new(workspace: Workspace, x: f32, ink: Ink, calling: bool, now: Instant) -> Self {
        let mut scale = Eased::lasting(ENTER_SCALE, DURATION_PANEL, now);
        scale.set_target(1.0, now);
        let mut opacity = Eased::lasting(0.0, DURATION_FADE, now);
        opacity.set_target(1.0, now);
        Self {
            workspace,
            x: Eased::lasting(x, DURATION_PANEL, now),
            scale,
            opacity,
            ink: Fade::new(ink, DURATION_COLOR, now),
            dot: Eased::lasting(if calling { 1.0 } else { 0.0 }, DURATION_FADE, now),
        }
    }

    pub fn id(&self) -> WorkspaceId {
        self.workspace.id
    }

    fn moving(&self, now: Instant) -> bool {
        !(self.x.settled(now)
            && self.scale.settled(now)
            && self.opacity.settled(now)
            && self.ink.settled(now)
            && self.dot.settled(now))
    }
}

/// Whether a workspace's dot shows: one you're on needs no reminder, and a
/// special one's windows wait there anyway.
fn calling(workspace: &Workspace) -> bool {
    workspace.urgent && !workspace.active && !workspace.special
}

fn ink(workspace: &Workspace, output_focused: bool) -> Ink {
    if workspace.active && output_focused {
        Ink::OnAccent
    } else if calling(workspace) {
        Ink::Calling
    } else if workspace.special && !workspace.active && !workspace.shown_elsewhere {
        Ink::Dim
    } else {
        Ink::Plain
    }
}

/// The distance from one number's left edge to the next one's.
fn step() -> f32 {
    f32::from(WORKSPACE_SIZE + WORKSPACE_GAP)
}

/// Every number of one screen's list, with the capsule behind the shown
/// one.
#[derive(Debug, Clone, PartialEq)]
pub struct Numbers {
    list: Vec<Number>,
    special_count: usize,
    output_focused: bool,
    /// The capsule's place, and whether there is a shown workspace for it.
    capsule_x: Eased,
    capsule_shown: bool,
    /// Whether the capsule is the accent (the output has the focus).
    capsule_accent: Fade<bool>,
    /// How wide the list is: it glides as numbers come and go.
    width: Eased,
}

impl Numbers {
    pub fn new(now: Instant) -> Self {
        Self {
            list: Vec::new(),
            special_count: 0,
            output_focused: false,
            capsule_x: Eased::lasting(0.0, DURATION_PANEL, now),
            capsule_shown: false,
            capsule_accent: Fade::new(false, DURATION_COLOR, now),
            width: Eased::lasting(0.0, DURATION_PANEL, now),
        }
    }

    /// Shows `workspaces` (from [`shown`]), keeping the numbers whose
    /// workspace stays.
    pub fn set(&mut self, workspaces: Vec<Workspace>, output_focused: bool, now: Instant) {
        self.output_focused = output_focused;
        self.special_count = workspaces.iter().filter(|workspace| workspace.special).count();
        let count = workspaces.len();
        let group_gap = self.group_gap(count);
        let mut old = std::mem::take(&mut self.list);
        let mut active = None;
        for (index, workspace) in workspaces.into_iter().enumerate() {
            let x = index as f32 * step() + if workspace.special { 0.0 } else { group_gap };
            let ink = ink(&workspace, output_focused);
            let dot = if calling(&workspace) { 1.0 } else { 0.0 };
            if workspace.active {
                active = Some(index);
            }
            let number = match old.iter().position(|number| number.id() == workspace.id) {
                Some(at) => {
                    let mut number = old.swap_remove(at);
                    number.x.set_target(x, now);
                    number.ink.set(ink, now);
                    number.dot.set_target(dot, now);
                    number.workspace = workspace;
                    number
                }
                None => Number::new(workspace, x, ink, dot > 0.0, now),
            };
            self.list.push(number);
        }
        let width = if count > 0 { count as f32 * step() - f32::from(WORKSPACE_GAP) + group_gap } else { 0.0 };
        self.width.set_target(width, now);
        self.capsule_accent.set(output_focused, now);
        match active {
            Some(index) => {
                let x = index as f32 * step() + if index >= self.special_count { group_gap } else { 0.0 };
                if self.capsule_shown {
                    self.capsule_x.set_target(x, now);
                } else {
                    // It shows up in place, not sliding in from the last.
                    self.capsule_x = Eased::lasting(x, DURATION_PANEL, now);
                }
                self.capsule_shown = true;
            }
            None => self.capsule_shown = false,
        }
    }

    /// The gap after the special workspaces, when there are any and
    /// numbers after them.
    fn group_gap(&self, count: usize) -> f32 {
        if self.special_count > 0 && self.special_count < count { f32::from(WORKSPACE_GROUP_GAP) } else { 0.0 }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Number> {
        self.list.iter()
    }

    /// The only special workspace shows a tilde, not its letter.
    pub fn lone_special(&self) -> bool {
        self.special_count == 1
    }

    pub fn output_focused(&self) -> bool {
        self.output_focused
    }

    pub fn width(&self, now: Instant) -> f32 {
        self.width.value(now)
    }

    /// Where the capsule is, if a workspace shows.
    pub fn capsule_x(&self, now: Instant) -> Option<f32> {
        self.capsule_shown.then(|| self.capsule_x.value(now))
    }

    pub fn capsule_accent(&self, now: Instant) -> (bool, bool, f32) {
        self.capsule_accent.at(now)
    }

    /// Whether anything is still on its way: the view asks for frames only
    /// then.
    pub fn moving(&self, now: Instant) -> bool {
        self.list.iter().any(|number| number.moving(now))
            || !(self.capsule_x.settled(now) && self.capsule_accent.settled(now) && self.width.settled(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_desktop_info::Output;

    fn workspace(id: WorkspaceId) -> Workspace {
        Workspace {
            id,
            name: id.to_string(),
            active: false,
            focused: false,
            urgent: false,
            empty: false,
            fullscreen: false,
            special: false,
            shown_elsewhere: false,
            active_window: None,
        }
    }

    fn active(id: WorkspaceId) -> Workspace {
        Workspace { active: true, ..workspace(id) }
    }

    fn special(id: WorkspaceId, name: &str) -> Workspace {
        Workspace { special: true, name: name.into(), ..workspace(id) }
    }

    fn desktop(workspaces: Vec<Workspace>) -> Desktop {
        Desktop {
            compositor: "niri".into(),
            capabilities: vec![Capability::Workspaces, Capability::FocusWorkspace],
            outputs: vec![Output { name: "DP-1".into(), focused: true, workspaces, layout: 0 }],
            focused_window: None,
            keyboard_layout: None,
        }
    }

    fn xs(numbers: &Numbers, now: Instant) -> Vec<f32> {
        numbers.iter().map(|number| number.x.value(now)).collect()
    }

    #[test]
    fn the_bar_lists_workspaces_with_windows_and_the_shown_one() {
        let empty = Workspace { empty: true, ..workspace(3) };
        let shown_empty = Workspace { empty: true, ..active(4) };
        let elsewhere = Workspace { empty: true, shown_elsewhere: true, ..special(-98, "music") };
        let listed = shown(&desktop(vec![elsewhere, workspace(1), empty, shown_empty]), "DP-1");
        let ids: Vec<_> = listed.iter().map(|workspace| workspace.id).collect();
        assert_eq!(ids, [-98, 1, 4]);
        assert!(shown(&desktop(vec![workspace(1)]), "HDMI-A-1").is_empty());
    }

    #[test]
    fn a_compositor_without_workspaces_lists_none() {
        let mut desktop = desktop(vec![workspace(1)]);
        desktop.capabilities.clear();
        assert!(shown(&desktop, "DP-1").is_empty());
    }

    #[test]
    fn a_number_that_stays_glides_and_a_new_one_grows_in() {
        let start = Instant::now();
        let mut numbers = Numbers::new(start);
        numbers.set(vec![workspace(1), active(3)], true, start);
        let settled = start + DURATION_PANEL;
        assert!(!numbers.moving(settled));
        assert_eq!(xs(&numbers, settled), [0.0, step()]);

        numbers.set(vec![workspace(1), workspace(2), active(3)], true, settled);
        assert_eq!(xs(&numbers, settled), [0.0, step(), step()], "3 starts from where it was");
        assert_eq!(numbers.iter().nth(1).map(|number| number.scale.value(settled)), Some(ENTER_SCALE));
        let after = settled + DURATION_PANEL;
        assert_eq!(xs(&numbers, after), [0.0, step(), 2.0 * step()]);
        assert!(!numbers.moving(after));
    }

    #[test]
    fn the_list_is_as_wide_as_its_numbers_and_glides_there() {
        let start = Instant::now();
        let mut numbers = Numbers::new(start);
        numbers.set(vec![active(1), workspace(2)], false, start);
        let two = 2.0 * step() - f32::from(WORKSPACE_GAP);
        assert_eq!(numbers.width(start), 0.0);
        assert_eq!(numbers.width(start + DURATION_PANEL), two);
        numbers.set(Vec::new(), false, start + DURATION_PANEL);
        assert_eq!(numbers.width(start + 2 * DURATION_PANEL), 0.0);
        assert_eq!(numbers.iter().count(), 0);
    }

    #[test]
    fn specials_lead_a_little_apart_and_the_capsule_follows_the_shown_one() {
        let start = Instant::now();
        let mut numbers = Numbers::new(start);
        numbers.set(vec![special(-98, "scratch"), workspace(1), active(2)], true, start);
        let gap = f32::from(WORKSPACE_GROUP_GAP);
        assert_eq!(xs(&numbers, start), [0.0, step() + gap, 2.0 * step() + gap]);
        assert_eq!(numbers.capsule_x(start), Some(2.0 * step() + gap), "in place at once");
        assert!(numbers.lone_special());

        numbers.set(vec![special(-98, "scratch"), active(1), workspace(2)], true, start);
        assert_eq!(numbers.capsule_x(start), Some(2.0 * step() + gap));
        assert_eq!(numbers.capsule_x(start + DURATION_PANEL), Some(step() + gap));

        numbers.set(vec![special(-98, "scratch"), workspace(1)], true, start + DURATION_PANEL);
        assert_eq!(numbers.capsule_x(start + DURATION_PANEL), None);
    }

    #[test]
    fn only_specials_need_no_gap() {
        let start = Instant::now();
        let mut numbers = Numbers::new(start);
        numbers.set(vec![special(-98, "a"), special(-97, "b")], false, start);
        assert_eq!(xs(&numbers, start), [0.0, step()]);
        assert!(!numbers.lone_special());
    }

    #[test]
    fn the_ink_says_what_a_number_means() {
        let urgent = Workspace { urgent: true, ..workspace(4) };
        assert_eq!(ink(&active(1), true), Ink::OnAccent);
        assert_eq!(ink(&active(1), false), Ink::Plain);
        assert_eq!(ink(&urgent, true), Ink::Calling);
        assert_eq!(ink(&Workspace { urgent: true, ..active(4) }, false), Ink::Plain, "no reminder where you are");
        assert_eq!(ink(&special(-98, "a"), true), Ink::Dim);
        assert_eq!(ink(&Workspace { shown_elsewhere: true, ..special(-98, "a") }, true), Ink::Plain);
        assert!(calling(&urgent));
        assert!(!calling(&Workspace { urgent: true, ..special(-98, "a") }));
    }

    #[test]
    fn a_change_of_focus_fades_the_capsule_and_the_shown_number() {
        let start = Instant::now();
        let mut numbers = Numbers::new(start);
        numbers.set(vec![active(1)], false, start);
        let later = start + DURATION_PANEL;
        numbers.set(vec![active(1)], true, later);
        assert_eq!(numbers.capsule_accent(later), (false, true, 0.0));
        let number = numbers.iter().next().cloned().unwrap();
        assert_eq!(number.ink.at(later + DURATION_COLOR), (Ink::Plain, Ink::OnAccent, 1.0));
        assert!(!numbers.moving(later + DURATION_COLOR));
    }
}
