use crate::tokens::{DOT_GAP, DOT_SIZE, FIELD_WIDTH, SHAKE, SHAKE_SWINGS};
use gpui::Pixels;
use nova_look::tokens::{ICON_SIZE, PAD};
use std::f32::consts::PI;
use std::time::{Duration, Instant};

pub const PLACEHOLDER: &str = "Password";
const WRONG: &str = "Wrong password";
const TOO_MANY_TRIES: &str = "Too many tries";
const UNCHECKED: &str = "Couldn't check the password";

const FIRST_PRINTABLE: char = ' ';
const DELETE: char = '\u{7f}';

#[derive(Debug, PartialEq, Eq)]
pub enum Key<'a> {
    Submit,
    Clear,
    Erase,
    Type(&'a str),
    Ignored,
}

impl<'a> Key<'a> {
    pub fn of(name: &str, typed: Option<&'a str>, shortcut: bool) -> Self {
        match (name, typed) {
            ("enter", _) => Self::Submit,
            ("escape", _) => Self::Clear,
            ("backspace", _) => Self::Erase,
            (_, Some(text)) if !shortcut && is_printable(text) => Self::Type(text),
            _ => Self::Ignored,
        }
    }
}

fn is_printable(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|character| character >= FIRST_PRINTABLE && character != DELETE)
}

#[derive(Default)]
pub struct Secret(String);

impl Secret {
    pub fn len(&self) -> usize {
        self.0.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn push(&mut self, text: &str) {
        self.0.push_str(text);
    }

    pub fn pop(&mut self) {
        self.0.pop();
    }

    pub fn clear(&mut self) {
        drop(self.take());
    }

    pub fn take(&mut self) -> Secret {
        Secret(std::mem::take(&mut self.0))
    }

    pub fn to_c_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.0.len() + 1);
        bytes.extend_from_slice(self.0.as_bytes());
        bytes.push(0);
        bytes
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(std::mem::take(&mut self.0).into_bytes());
    }
}

pub fn wipe(mut bytes: Vec<u8>) {
    bytes.clear();
    bytes.resize(bytes.capacity(), 0);
    std::hint::black_box(&bytes);
}

pub enum Verdict {
    Right,
    Wrong,
    TooManyTries,
    Unchecked(String),
}

impl Verdict {
    pub fn refusal(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::Right => None,
            Self::Wrong => Some((WRONG, "wrong password")),
            Self::TooManyTries => Some((TOO_MANY_TRIES, "PAM's tries ran out")),
            Self::Unchecked(why) => Some((UNCHECKED, why)),
        }
    }
}

pub fn dots(typed: usize) -> usize {
    let room = FIELD_WIDTH - PAD * 3.0 - ICON_SIZE;
    let fits = (room / (DOT_SIZE + DOT_GAP)).floor().max(0.0);
    typed.min(fits as usize)
}

pub fn progress(since: Instant, lasting: Duration, now: Instant) -> f32 {
    if lasting.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(since).as_secs_f32() / lasting.as_secs_f32()).min(1.0)
}

pub fn shake(progress: f32) -> Pixels {
    SHAKE * ((progress * PI * SHAKE_SWINGS).sin() * (1.0 - progress))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    #[test]
    fn keys_do_what_they_do_in_today_s_lock() {
        assert_eq!(Key::of("enter", Some("\n"), false), Key::Submit);
        assert_eq!(Key::of("escape", None, false), Key::Clear);
        assert_eq!(Key::of("backspace", None, false), Key::Erase);
        assert_eq!(Key::of("a", Some("a"), false), Key::Type("a"));
        assert_eq!(Key::of("space", Some(" "), false), Key::Type(" "));
        assert_eq!(Key::of("e", Some("é"), false), Key::Type("é"));
        assert_eq!(Key::of("a", None, false), Key::Ignored);
        assert_eq!(Key::of("l", Some("l"), true), Key::Ignored, "Super+L types nothing");
        assert_eq!(Key::of("enter", None, true), Key::Submit);
        assert_eq!(Key::of("shift", None, false), Key::Ignored);
        assert_eq!(Key::of("tab", Some("\t"), false), Key::Ignored);
        assert_eq!(Key::of("delete", Some("\u{7f}"), false), Key::Ignored);
    }

    #[test]
    fn a_secret_counts_characters_and_empties_when_taken() {
        let mut typed = Secret::default();
        typed.push("pä");
        typed.push("ß");
        assert_eq!(typed.len(), 3);
        typed.pop();
        assert_eq!(typed.len(), 2);
        let taken = typed.take();
        assert!(typed.is_empty());
        let bytes = taken.to_c_bytes();
        assert_eq!(bytes, "pä\0".as_bytes());
        assert_eq!(bytes.capacity(), bytes.len(), "no room left for C's copy to move it");
    }

    #[test]
    fn the_field_shows_only_the_dots_that_fit() {
        assert_eq!(dots(0), 0);
        assert_eq!(dots(5), 5);
        assert_eq!(dots(40), 16);
    }

    #[test]
    fn a_shake_swings_both_ways_and_ends_where_it_started() {
        assert_eq!(shake(0.0), px(0.0));
        assert!(shake(1.0).abs() < px(0.001));
        assert!(shake(0.125) > px(6.0), "{:?}", shake(0.125));
        assert!(shake(0.375) < px(-4.0), "{:?}", shake(0.375));
        assert!(shake(0.625).abs() < shake(0.125).abs());
    }

    #[test]
    fn progress_runs_evenly_and_stops_at_the_end() {
        let start = Instant::now();
        let lasting = Duration::from_millis(400);
        assert_eq!(progress(start, lasting, start), 0.0);
        assert!((progress(start, lasting, start + Duration::from_millis(100)) - 0.25).abs() < 1e-6);
        assert_eq!(progress(start, lasting, start + Duration::from_secs(2)), 1.0);
    }
}
