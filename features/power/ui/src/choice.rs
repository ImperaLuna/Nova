use crate::tokens::DURATION_CONFIRM;
use nova_power_api::{PowerAction, PowerOption};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub action: PowerAction,
    pub label: &'static str,
    pub doing: &'static str,
    pub key: char,
    pub confirm: bool,
    pub leaves: bool,
}

pub const ENTRIES: [Entry; 5] = [
    Entry { action: PowerAction::Lock, label: "Lock", doing: "Locking", key: 'l', confirm: false, leaves: false },
    Entry {
        action: PowerAction::LogOut,
        label: "Log out",
        doing: "Logging out",
        key: 'o',
        confirm: true,
        leaves: true,
    },
    Entry {
        action: PowerAction::Suspend,
        label: "Suspend",
        doing: "Suspending",
        key: 's',
        confirm: false,
        leaves: false,
    },
    Entry { action: PowerAction::Reboot, label: "Restart", doing: "Restarting", key: 'r', confirm: true, leaves: true },
    Entry {
        action: PowerAction::PowerOff,
        label: "Shut down",
        doing: "Shutting down",
        key: 'p',
        confirm: true,
        leaves: true,
    },
];

pub fn entry(action: PowerAction) -> &'static Entry {
    ENTRIES.iter().find(|entry| entry.action == action).unwrap_or(&ENTRIES[0])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Nothing,
    Run(PowerAction),
    Leave(PowerAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Next,
    Previous,
    Enter,
    Escape,
    Letter(char),
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Passed,
    Taken(Pick),
}

#[derive(Debug, Clone)]
pub struct Choice {
    options: Vec<PowerOption>,
    pub selected: PowerAction,
    armed: Option<(PowerAction, Instant)>,
    pub running: Option<PowerAction>,
    pub leaving: Option<PowerAction>,
    pub problem: String,
}

impl Default for Choice {
    fn default() -> Self {
        Self {
            options: Vec::new(),
            selected: PowerAction::Lock,
            armed: None,
            running: None,
            leaving: None,
            problem: String::new(),
        }
    }
}

impl Choice {
    pub fn set_options(&mut self, options: Vec<PowerOption>) {
        self.options = options;
        if !self.available(self.selected)
            && self.armed.is_none()
            && self.running.is_none()
            && let Some(action) = ENTRIES.iter().map(|entry| entry.action).find(|action| self.available(*action))
        {
            self.selected = action;
        }
    }

    fn option(&self, action: PowerAction) -> Option<&PowerOption> {
        self.options.iter().find(|option| option.action == action)
    }

    pub fn available(&self, action: PowerAction) -> bool {
        self.option(action).is_none_or(|option| option.available)
    }

    pub fn reason(&self, action: PowerAction) -> &str {
        self.option(action).and_then(|option| option.reason.as_deref()).unwrap_or_default()
    }

    pub fn armed(&self, now: Instant) -> Option<PowerAction> {
        self.armed
            .filter(|(_, since)| now.saturating_duration_since(*since) < DURATION_CONFIRM)
            .map(|(action, _)| action)
    }

    pub fn armed_left(&self, now: Instant) -> f32 {
        let Some((_, since)) = self.armed else { return 0.0 };
        let gone = now.saturating_duration_since(since).as_secs_f32() / DURATION_CONFIRM.as_secs_f32();
        (1.0 - gone).clamp(0.0, 1.0)
    }

    pub fn seconds_left(&self, now: Instant) -> u64 {
        (self.armed_left(now) * DURATION_CONFIRM.as_secs_f32()).ceil() as u64
    }

    pub fn reset(&mut self) {
        if self.leaving.take().is_some() {
            self.running = None;
        }
        self.disarm();
        self.selected = PowerAction::Lock;
        self.problem.clear();
    }

    pub fn disarm(&mut self) {
        self.armed = None;
    }

    pub fn hover(&mut self, action: PowerAction, now: Instant) {
        if self.armed(now).is_none() && self.running.is_none() && self.available(action) {
            self.selected = action;
        }
    }

    pub fn step(&mut self, by: isize) {
        if self.running.is_some() {
            return;
        }
        self.disarm();
        let count = ENTRIES.len() as isize;
        let at = ENTRIES.iter().position(|entry| entry.action == self.selected).unwrap_or(0) as isize;
        let next = (1..=count)
            .map(|offset| ENTRIES[(at + by * offset).rem_euclid(count) as usize].action)
            .find(|action| self.available(*action));
        if let Some(next) = next {
            self.selected = next;
        }
    }

    pub fn pick(&mut self, action: PowerAction, now: Instant) -> Pick {
        if self.running.is_some() || !self.available(action) {
            return Pick::Nothing;
        }
        self.selected = action;
        let entry = entry(action);
        if entry.confirm && self.armed(now) != Some(action) {
            self.armed = Some((action, now));
            return Pick::Nothing;
        }
        self.disarm();
        self.running = Some(action);
        if entry.leaves {
            self.leaving = Some(action);
            return Pick::Leave(action);
        }
        Pick::Run(action)
    }

    pub fn left(&mut self) -> Option<PowerAction> {
        self.leaving.take()
    }

    pub fn finished(&mut self, problem: Option<String>) {
        self.running = None;
        self.problem = problem.unwrap_or_default();
    }

    pub fn key(&mut self, key: Key, repeat: bool, now: Instant) -> Answer {
        if repeat && !matches!(key, Key::Next | Key::Previous) {
            return if key == Key::Other { Answer::Passed } else { Answer::Taken(Pick::Nothing) };
        }
        match key {
            Key::Escape if self.armed(now).is_some() => {
                self.disarm();
                Answer::Taken(Pick::Nothing)
            }
            Key::Escape | Key::Other => Answer::Passed,
            Key::Next => {
                self.step(1);
                Answer::Taken(Pick::Nothing)
            }
            Key::Previous => {
                self.step(-1);
                Answer::Taken(Pick::Nothing)
            }
            Key::Enter => Answer::Taken(self.pick(self.selected, now)),
            Key::Letter(letter) => match ENTRIES.iter().find(|entry| entry.key == letter) {
                Some(entry) => Answer::Taken(self.pick(entry.action, now)),
                None => Answer::Passed,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn dimmed(action: PowerAction) -> PowerOption {
        PowerOption { action, available: false, reason: Some("Not allowed".into()) }
    }

    #[test]
    fn it_starts_on_lock_and_lock_runs_at_once() {
        let mut choice = Choice::default();
        let now = Instant::now();
        assert_eq!(choice.selected, PowerAction::Lock);
        assert_eq!(choice.key(Key::Enter, false, now), Answer::Taken(Pick::Run(PowerAction::Lock)));
        assert_eq!(choice.running, Some(PowerAction::Lock));
        assert_eq!(choice.pick(PowerAction::Suspend, now), Pick::Nothing, "nothing else while one runs");
        choice.finished(Some("novad: the lock screen isn't ready".into()));
        assert_eq!((choice.running, choice.problem.as_str()), (None, "novad: the lock screen isn't ready"));
    }

    #[test]
    fn ending_the_session_takes_a_second_pick_in_time() {
        let mut choice = Choice::default();
        let start = Instant::now();
        assert_eq!(choice.pick(PowerAction::PowerOff, start), Pick::Nothing, "the first pick arms");
        assert_eq!(choice.armed(start), Some(PowerAction::PowerOff));
        let halfway = start + DURATION_CONFIRM / 2;
        assert!((choice.armed_left(halfway) - 0.5).abs() < 0.01);
        assert_eq!(choice.seconds_left(halfway), 2);
        assert_eq!(choice.seconds_left(start + Duration::from_millis(100)), 4);
        assert_eq!(choice.pick(PowerAction::PowerOff, halfway), Pick::Leave(PowerAction::PowerOff));
        assert_eq!(choice.armed(halfway), None);
        assert_eq!(choice.left(), Some(PowerAction::PowerOff), "it runs once the menu has closed");
        assert_eq!(choice.left(), None, "once");
    }

    #[test]
    fn an_armed_action_lets_go_when_its_time_is_up() {
        let mut choice = Choice::default();
        let start = Instant::now();
        choice.pick(PowerAction::Reboot, start);
        let late = start + DURATION_CONFIRM;
        assert_eq!(choice.armed(late), None);
        assert_eq!(choice.armed_left(late), 0.0);
        assert_eq!(choice.pick(PowerAction::Reboot, late), Pick::Nothing, "it arms again instead");
    }

    #[test]
    fn opening_again_before_it_closed_keeps_the_session() {
        let mut choice = Choice::default();
        let now = Instant::now();
        choice.pick(PowerAction::LogOut, now);
        assert_eq!(choice.pick(PowerAction::LogOut, now), Pick::Leave(PowerAction::LogOut));
        choice.reset();
        assert_eq!((choice.running, choice.left()), (None, None));
        assert_eq!(choice.selected, PowerAction::Lock);
    }

    #[test]
    fn options_that_dim_the_selected_row_move_the_selection_to_one_that_runs() {
        let mut choice = Choice::default();
        assert_eq!(choice.selected, PowerAction::Lock);
        choice.set_options(vec![dimmed(PowerAction::Lock)]);
        assert_eq!(choice.selected, PowerAction::LogOut, "off the dimmed Lock to the next that runs");
        let now = Instant::now();
        choice.pick(PowerAction::Reboot, now);
        choice.set_options(vec![dimmed(PowerAction::Reboot)]);
        assert_eq!(choice.selected, PowerAction::Reboot, "an armed row is left where it is");
    }

    #[test]
    fn the_selection_wraps_and_skips_what_can_t_run() {
        let mut choice = Choice::default();
        choice.set_options(vec![dimmed(PowerAction::LogOut), dimmed(PowerAction::Suspend)]);
        let now = Instant::now();
        choice.key(Key::Next, false, now);
        assert_eq!(choice.selected, PowerAction::Reboot, "past the two dimmed rows");
        choice.key(Key::Previous, false, now);
        choice.key(Key::Previous, false, now);
        assert_eq!(choice.selected, PowerAction::PowerOff, "round from the top");
        assert_eq!(choice.pick(PowerAction::Suspend, now), Pick::Nothing, "a dimmed row never runs");
        assert_eq!(choice.reason(PowerAction::Suspend), "Not allowed");
        assert_eq!(choice.reason(PowerAction::Lock), "");
        choice.hover(PowerAction::LogOut, now);
        assert_eq!(choice.selected, PowerAction::PowerOff, "hovering a dimmed row selects nothing");
    }

    #[test]
    fn the_pointer_selects_only_while_nothing_is_armed() {
        let mut choice = Choice::default();
        let now = Instant::now();
        choice.hover(PowerAction::Suspend, now);
        assert_eq!(choice.selected, PowerAction::Suspend);
        choice.pick(PowerAction::Reboot, now);
        choice.hover(PowerAction::Lock, now);
        assert_eq!(choice.selected, PowerAction::Reboot);
    }

    #[test]
    fn letters_pick_their_rows_and_esc_disarms_before_it_closes() {
        let mut choice = Choice::default();
        let now = Instant::now();
        assert_eq!(choice.key(Key::Letter('r'), false, now), Answer::Taken(Pick::Nothing));
        assert_eq!(choice.armed(now), Some(PowerAction::Reboot));
        assert_eq!(choice.key(Key::Escape, false, now), Answer::Taken(Pick::Nothing), "Esc disarms first");
        assert_eq!(choice.armed(now), None);
        assert_eq!(choice.key(Key::Escape, false, now), Answer::Passed, "then closes the menu");
        assert_eq!(choice.key(Key::Letter('x'), false, now), Answer::Passed);
        assert_eq!(choice.key(Key::Letter('s'), false, now), Answer::Taken(Pick::Run(PowerAction::Suspend)));
    }

    #[test]
    fn a_held_key_moves_but_never_picks() {
        let mut choice = Choice::default();
        let now = Instant::now();
        assert_eq!(choice.key(Key::Letter('p'), false, now), Answer::Taken(Pick::Nothing));
        assert_eq!(choice.key(Key::Letter('p'), true, now), Answer::Taken(Pick::Nothing), "held: no confirm");
        assert_eq!(choice.running, None);
        assert_eq!(choice.key(Key::Enter, true, now), Answer::Taken(Pick::Nothing));
        choice.key(Key::Next, true, now);
        assert_eq!(choice.selected, PowerAction::Lock, "a held arrow moves, round to the top");
    }

    #[test]
    fn every_action_has_one_row_and_one_key() {
        for action in PowerAction::ALL {
            assert_eq!(ENTRIES.iter().filter(|entry| entry.action == action).count(), 1, "{action:?}");
        }
        let mut keys: Vec<char> = ENTRIES.iter().map(|entry| entry.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), ENTRIES.len(), "every row has a distinct key");
        let leaves: Vec<_> = ENTRIES.iter().filter(|entry| entry.leaves).map(|entry| entry.action).collect();
        assert_eq!(leaves, [PowerAction::LogOut, PowerAction::Reboot, PowerAction::PowerOff]);
        assert!(ENTRIES.iter().all(|entry| entry.leaves == entry.confirm));
    }
}
