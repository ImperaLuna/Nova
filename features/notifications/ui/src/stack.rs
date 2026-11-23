use crate::logic;
use crate::tokens::{SHOWN, STAGGER, TIMEOUT};
use nova_look::motion::Eased;
use nova_notifications_api::Notification;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leaving {
    Fold,
    Away,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Countdown {
    duration: Duration,
    spent: Duration,
    since: Option<Instant>,
}

impl Countdown {
    fn held(duration: Duration) -> Self {
        Self { duration, spent: Duration::ZERO, since: None }
    }

    fn run(&mut self, now: Instant) {
        if self.since.is_none() && !self.duration.is_zero() {
            self.since = Some(now);
        }
    }

    fn hold(&mut self, now: Instant) {
        if let Some(since) = self.since.take() {
            self.spent = (self.spent + now.saturating_duration_since(since)).min(self.duration);
        }
    }

    fn restart(&mut self, duration: Duration, now: Instant) {
        let running = self.since.is_some();
        *self = Self::held(duration);
        if running {
            self.run(now);
        }
    }

    fn spent(&self, now: Instant) -> Duration {
        self.spent + self.since.map_or(Duration::ZERO, |since| now.saturating_duration_since(since))
    }

    pub fn remaining(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        (1.0 - self.spent(now).as_secs_f32() / self.duration.as_secs_f32()).max(0.0)
    }

    pub fn shown(&self) -> bool {
        !self.duration.is_zero()
    }

    pub fn running(&self) -> bool {
        self.since.is_some()
    }

    fn expired(&self, now: Instant) -> bool {
        !self.duration.is_zero() && self.spent(now) >= self.duration
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub key: u64,
    pub notification: Notification,
    listed: bool,
    pub leaving: Option<Leaving>,
    pub room: Eased,
    pub presence: Eased,
    opens_at: Option<Instant>,
    pub countdown: Countdown,
    pub held: bool,
}

impl Entry {
    fn refresh(&mut self, item: &Notification, now: Instant) {
        self.listed = true;
        if *item != self.notification {
            let duration = timeout(item);
            if duration != timeout(&self.notification) {
                self.countdown.restart(duration, now);
            }
            self.notification = item.clone();
        }
    }

    fn new(key: u64, notification: Notification, listed: bool, now: Instant) -> Self {
        let countdown = Countdown::held(timeout(&notification));
        Self {
            key,
            notification,
            listed,
            leaving: None,
            room: Eased::at(0.0, now),
            presence: Eased::at(0.0, now),
            opens_at: None,
            countdown,
            held: false,
        }
    }

    pub fn open(&self, now: Instant) -> bool {
        self.leaving.is_none() && self.opens_at.is_some_and(|at| now >= at)
    }

    fn moving(&self, now: Instant) -> bool {
        !self.room.settled(now) || !self.presence.settled(now)
    }

    fn keep_time(&mut self, now: Instant) {
        if self.open(now) && !self.held {
            self.countdown.run(now);
        } else {
            self.countdown.hold(now);
        }
    }
}

pub fn timeout(notification: &Notification) -> Duration {
    logic::timeout(notification, TIMEOUT)
}

#[derive(Debug, Clone)]
struct Waiting {
    notification: Notification,
    listed: bool,
}

#[derive(Debug, Default)]
pub struct Stack {
    entries: Vec<Entry>,
    waiting: Vec<Waiting>,
    next_key: u64,
    arriving: Vec<u64>,
}

impl Stack {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    pub fn busy(&self) -> bool {
        !self.entries.is_empty() || !self.waiting.is_empty()
    }

    pub fn moving(&self, now: Instant) -> bool {
        self.entries.iter().any(|entry| entry.moving(now) || entry.opens_at.is_some_and(|at| at > now))
    }

    pub fn arrivals(&self) -> &[u64] {
        &self.arriving
    }

    pub fn counting(&self) -> bool {
        self.entries.iter().any(|entry| entry.countdown.running())
    }

    fn shown(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|entry| entry.leaving.is_none())
    }

    fn key(&mut self) -> u64 {
        self.next_key += 1;
        self.next_key
    }

    pub fn add(&mut self, notification: Notification, now: Instant) {
        let id = notification.id;
        if let Some(entry) =
            self.entries.iter_mut().find(|entry| entry.leaving.is_none() && entry.notification.id == id)
        {
            entry.countdown.restart(timeout(&notification), now);
            entry.notification = notification;
            return;
        }
        if let Some(held) = self.waiting.iter_mut().find(|held| held.notification.id == id) {
            held.notification = notification;
            return;
        }
        if self.shown().count() + 1 > SHOWN
            && let Some(oldest) = self.shown().last().map(|entry| entry.key)
        {
            let taken = self.take(oldest, Leaving::Fold, now);
            if let Some(entry) = taken {
                self.waiting.insert(0, Waiting { notification: entry.notification, listed: entry.listed });
            }
        }
        let key = self.key();
        self.entries.insert(0, Entry::new(key, notification, false, now));
        self.arriving.push(key);
    }

    pub fn open_arrivals(&mut self, now: Instant) {
        let arriving = std::mem::take(&mut self.arriving);
        let mut delay = Duration::ZERO;
        for key in arriving.into_iter().rev() {
            if let Some(entry) = self.entries.iter_mut().find(|entry| entry.key == key) {
                entry.room.set_target(1.0, now);
                entry.opens_at = Some(now + delay);
                entry.presence.set_target(1.0, now + delay);
                delay += STAGGER;
            }
        }
    }

    pub fn remove(&mut self, key: u64, how: Leaving, now: Instant) {
        if self.take(key, how, now).is_none() {
            return;
        }
        if !self.waiting.is_empty() && self.shown().count() < SHOWN {
            let next = self.waiting.remove(0);
            let key = self.key();
            self.entries.push(Entry::new(key, next.notification, next.listed, now));
            self.arriving.push(key);
        }
    }

    fn take(&mut self, key: u64, how: Leaving, now: Instant) -> Option<Entry> {
        let entry = self.entries.iter_mut().find(|entry| entry.key == key && entry.leaving.is_none())?;
        self.arriving.retain(|arriving| *arriving != key);
        let before = entry.clone();
        entry.leaving = Some(how);
        entry.room.set_target(0.0, now);
        entry.presence.set_target(0.0, now);
        entry.countdown.hold(now);
        Some(before)
    }

    pub fn sync(&mut self, items: &[Notification], now: Instant) {
        let find = |id: u32| items.iter().find(|item| item.id == id);
        for held in &mut self.waiting {
            if let Some(item) = find(held.notification.id) {
                held.listed = true;
                held.notification = item.clone();
            }
        }
        self.waiting.retain(|held| !held.listed || find(held.notification.id).is_some());
        let mut gone = Vec::new();
        for entry in self.entries.iter_mut().filter(|entry| entry.leaving.is_none()) {
            match find(entry.notification.id) {
                Some(item) => entry.refresh(item, now),
                None if entry.listed => gone.push(entry.key),
                None => {}
            }
        }
        for key in gone {
            self.remove(key, Leaving::Away, now);
        }
    }

    pub fn clear(&mut self, now: Instant) {
        self.waiting.clear();
        let keys: Vec<u64> = self.shown().map(|entry| entry.key).collect();
        for key in keys {
            self.take(key, Leaving::Fold, now);
        }
    }

    pub fn set_held(&mut self, key: u64, held: bool, now: Instant) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.key == key) {
            entry.held = held;
            entry.keep_time(now);
        }
    }

    pub fn step(&mut self, now: Instant) {
        let expired: Vec<u64> = self
            .entries
            .iter_mut()
            .filter(|entry| entry.leaving.is_none())
            .filter_map(|entry| {
                entry.keep_time(now);
                entry.countdown.expired(now).then_some(entry.key)
            })
            .collect();
        for key in expired {
            self.remove(key, Leaving::Fold, now);
        }
        self.entries
            .retain(|entry| entry.leaving.is_none() || !entry.presence.settled(now) || !entry.room.settled(now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::TIMEOUT;
    use nova_look::tokens::DURATION_PANEL;
    use nova_notifications_api::Urgency;

    fn notification(id: u32) -> Notification {
        Notification {
            id,
            app: "Firefox".into(),
            icon: None,
            app_id: None,
            summary: format!("Download {id}"),
            body: String::new(),
            image: None,
            actions: vec![],
            has_default: true,
            urgency: Urgency::Normal,
            progress: None,
            timeout_ms: None,
            time: 1_761_400_000,
            unread: true,
            live: true,
            device: None,
        }
    }

    fn ids(stack: &Stack) -> Vec<u32> {
        stack.shown().map(|entry| entry.notification.id).collect()
    }

    fn later(now: Instant) -> Instant {
        now + DURATION_PANEL + STAGGER * SHOWN as u32
    }

    #[test]
    fn several_at_once_open_top_first_a_stagger_apart() {
        let now = Instant::now();
        let mut stack = Stack::default();
        for id in 1..=3 {
            stack.add(notification(id), now);
        }
        stack.open_arrivals(now);
        assert_eq!(ids(&stack), [3, 2, 1], "the newest is on top");
        let opens: Vec<Instant> = stack.entries().iter().filter_map(|entry| entry.opens_at).collect();
        assert_eq!(opens, [now, now + STAGGER, now + STAGGER * 2]);
        assert!(stack.moving(now));
        let done = later(now);
        stack.step(done);
        assert!(!stack.moving(done) && stack.entries().iter().all(|entry| entry.open(done)));
    }

    #[test]
    fn a_new_one_takes_no_room_and_stands_still_until_it_is_opened() {
        let now = Instant::now();
        let mut stack = Stack::default();
        stack.add(notification(1), now);
        let key = stack.entries()[0].key;
        assert_eq!(stack.arrivals(), [key]);
        let waited = later(now);
        stack.step(waited);
        assert!(stack.busy() && !stack.moving(waited) && !stack.counting());
        assert_eq!(stack.entries()[0].room.value(waited), 0.0);
        stack.open_arrivals(waited);
        stack.step(waited);
        assert!(stack.arrivals().is_empty() && stack.moving(waited) && stack.counting());
        stack.add(notification(2), waited);
        stack.clear(waited);
        assert!(stack.arrivals().is_empty());
    }

    #[test]
    fn past_the_room_for_them_the_oldest_waits_and_comes_back_when_one_goes() {
        let now = Instant::now();
        let mut stack = Stack::default();
        for id in 1..=SHOWN as u32 + 1 {
            stack.add(notification(id), now);
        }
        stack.open_arrivals(now);
        assert_eq!(stack.shown().count(), SHOWN);
        assert_eq!(stack.waiting(), 1);
        let top = stack.entries()[0].key;
        stack.remove(top, Leaving::Away, later(now));
        assert_eq!(stack.waiting(), 0);
        let back = stack.shown().last().expect("the one waiting came back");
        assert_eq!(back.notification.id, 1);
        assert_eq!(stack.arrivals(), [back.key], "to be opened like a new one");
    }

    #[test]
    fn its_time_runs_once_open_stands_while_held_and_then_it_folds_away() {
        let now = Instant::now();
        let mut stack = Stack::default();
        stack.add(notification(1), now);
        stack.open_arrivals(now);
        stack.step(now);
        let key = stack.entries()[0].key;
        stack.set_held(key, true, now + TIMEOUT / 2);
        stack.step(now + TIMEOUT * 3);
        assert_eq!(ids(&stack), [1]);
        stack.set_held(key, false, now + TIMEOUT * 3);
        stack.step(now + TIMEOUT * 3 + TIMEOUT / 2);
        assert_eq!(stack.entries()[0].leaving, Some(Leaving::Fold));
        stack.step(later(now + TIMEOUT * 4));
        assert!(!stack.busy());
    }

    #[test]
    fn critical_ones_and_running_progress_stay_until_closed() {
        let mut critical = notification(1);
        critical.urgency = Urgency::Critical;
        assert_eq!(timeout(&critical), Duration::ZERO);
        let mut progress = notification(2);
        progress.progress = Some(40);
        assert_eq!(timeout(&progress), Duration::ZERO);
        progress.progress = Some(100);
        assert_eq!(timeout(&progress), TIMEOUT);
        let mut asked = notification(3);
        asked.timeout_ms = Some(2000);
        assert_eq!(timeout(&asked), Duration::from_secs(2));
    }

    #[test]
    fn a_listed_pop_up_goes_once_the_list_loses_it_and_shows_it_as_it_is_there() {
        let now = Instant::now();
        let mut stack = Stack::default();
        stack.add(notification(1), now);
        stack.add(notification(2), now);
        stack.open_arrivals(now);
        stack.sync(&[notification(2)], now);
        assert_eq!(ids(&stack), [2, 1]);
        let mut changed = notification(1);
        changed.summary = "Done".into();
        stack.sync(&[notification(2), changed], now);
        assert_eq!(stack.entries()[1].notification.summary, "Done");
        stack.sync(&[notification(1)], now);
        assert_eq!(ids(&stack), [1], "2 was dismissed elsewhere");
        assert_eq!(stack.entries()[0].leaving, Some(Leaving::Away));
    }

    #[test]
    fn the_same_one_again_updates_its_pop_up_and_starts_its_time_over() {
        let now = Instant::now();
        let mut stack = Stack::default();
        stack.add(notification(1), now);
        stack.open_arrivals(now);
        stack.step(now);
        let half = now + TIMEOUT / 2;
        stack.step(half);
        let mut again = notification(1);
        again.summary = "Again".into();
        stack.add(again, half);
        assert_eq!(stack.entries().len(), 1);
        assert_eq!(stack.entries()[0].notification.summary, "Again");
        assert!((stack.entries()[0].countdown.remaining(half) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn clearing_folds_every_pop_up_away() {
        let now = Instant::now();
        let mut stack = Stack::default();
        for id in 1..=SHOWN as u32 + 2 {
            stack.add(notification(id), now);
        }
        stack.open_arrivals(now);
        stack.clear(now);
        assert_eq!(stack.waiting(), 0);
        assert!(stack.entries().iter().all(|entry| entry.leaving == Some(Leaving::Fold)));
        stack.step(later(now));
        assert!(!stack.busy());
    }
}
