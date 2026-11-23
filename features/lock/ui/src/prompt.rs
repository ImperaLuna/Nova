//! What the lock screen's prompt works out without drawing: which key does
//! what, the characters typed so far, how many dots fit, where a shake has
//! the field, and what a failed check says. `Authenticator.qml` and
//! `LockPrompt.qml` in today's shell.

use crate::tokens::{DOT_GAP, DOT_SIZE, FIELD_WIDTH, SHAKE, SHAKE_SWINGS};
use gpui::Pixels;
use nova_look::tokens::{ICON_SIZE, PAD};
use std::f32::consts::PI;
use std::time::{Duration, Instant};

/// The field's placeholder, and what it says under the field after a check
/// that didn't unlock.
pub const PLACEHOLDER: &str = "Password";
const WRONG: &str = "Wrong password";
const TOO_MANY_TRIES: &str = "Too many tries";
const UNCHECKED: &str = "Couldn't check the password";

/// The first character that is no control character, and the one above
/// the printable ASCII ones that is.
const FIRST_PRINTABLE: char = ' ';
const DELETE: char = '\u{7f}';

/// What a key does to the prompt.
#[derive(Debug, PartialEq, Eq)]
pub enum Key<'a> {
    /// Checks what was typed.
    Submit,
    /// Empties the field.
    Clear,
    /// Takes the last character back.
    Erase,
    /// Adds what the key types.
    Type(&'a str),
    /// Nothing: a modifier, an arrow, a shortcut.
    Ignored,
}

impl<'a> Key<'a> {
    /// `name` is GPUI's name for the key, `typed` the text it types, if any.
    pub fn of(name: &str, typed: Option<&'a str>) -> Self {
        match (name, typed) {
            ("enter", _) => Self::Submit,
            ("escape", _) => Self::Clear,
            ("backspace", _) => Self::Erase,
            (_, Some(text)) if is_printable(text) => Self::Type(text),
            _ => Self::Ignored,
        }
    }
}

fn is_printable(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|character| character >= FIRST_PRINTABLE && character != DELETE)
}

/// The password typed so far, or one on its way to the check. No `Debug`
/// and no `Display`, so it can't reach a log, and its bytes are overwritten
/// when it is emptied or dropped. (Copies the allocator left behind as the
/// text grew are beyond reach from here.)
#[derive(Default)]
pub struct Secret(String);

impl Secret {
    /// How many characters, for the dots.
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

    /// Hands the password on, leaving this empty.
    pub fn take(&mut self) -> Secret {
        Secret(std::mem::take(&mut self.0))
    }

    /// The bytes, for the check; the secret itself is spent.
    pub fn into_bytes(mut self) -> Vec<u8> {
        std::mem::take(&mut self.0).into_bytes()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(std::mem::take(&mut self.0).into_bytes());
    }
}

/// Overwrites a password's bytes before their memory is given back.
pub fn wipe(mut bytes: Vec<u8>) {
    bytes.fill(0);
    // Kept from being optimized away as a write nobody reads.
    std::hint::black_box(&bytes);
}

/// What the check of a password found.
pub enum Verdict {
    Right,
    Wrong,
    /// PAM counts tries and has had enough.
    TooManyTries,
    /// The check itself failed; the text is for the log.
    Unchecked(String),
}

impl Verdict {
    /// What the prompt says under the field, and the log beside it; `None`
    /// for the right password.
    pub fn refusal(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::Right => None,
            Self::Wrong => Some((WRONG, "wrong password")),
            Self::TooManyTries => Some((TOO_MANY_TRIES, "PAM's tries ran out")),
            Self::Unchecked(why) => Some((UNCHECKED, why)),
        }
    }
}

/// How many of `typed` dots the field shows: as many as fit before the
/// spinner's room.
pub fn dots(typed: usize) -> usize {
    let room = FIELD_WIDTH - PAD * 3.0 - ICON_SIZE;
    let fits = (room / (DOT_SIZE + DOT_GAP)).floor().max(0.0);
    typed.min(fits as usize)
}

/// How far along something that started at `since` and takes `lasting` is
/// at `now`, from 0 to 1, at an even pace (a QML animation's default).
pub fn progress(since: Instant, lasting: Duration, now: Instant) -> f32 {
    if lasting.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(since).as_secs_f32() / lasting.as_secs_f32()).min(1.0)
}

/// Where a shake has the field at `progress` (0 to 1): back and forth,
/// each swing smaller, ending where it started.
pub fn shake(progress: f32) -> Pixels {
    SHAKE * ((progress * PI * SHAKE_SWINGS).sin() * (1.0 - progress))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    #[test]
    fn keys_do_what_they_do_in_today_s_lock() {
        assert_eq!(Key::of("enter", Some("\n")), Key::Submit);
        assert_eq!(Key::of("escape", None), Key::Clear);
        assert_eq!(Key::of("backspace", None), Key::Erase);
        assert_eq!(Key::of("a", Some("a")), Key::Type("a"));
        assert_eq!(Key::of("space", Some(" ")), Key::Type(" "));
        assert_eq!(Key::of("e", Some("é")), Key::Type("é"));
        // A shortcut, a modifier alone, and keys that type a control character.
        assert_eq!(Key::of("a", None), Key::Ignored);
        assert_eq!(Key::of("shift", None), Key::Ignored);
        assert_eq!(Key::of("tab", Some("\t")), Key::Ignored);
        assert_eq!(Key::of("delete", Some("\u{7f}")), Key::Ignored);
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
        assert_eq!(taken.into_bytes(), "pä".as_bytes());
    }

    #[test]
    fn the_field_shows_only_the_dots_that_fit() {
        assert_eq!(dots(0), 0);
        assert_eq!(dots(5), 5);
        // (280 - 3 * 12 - 16) / (8 + 6) = 16.3
        assert_eq!(dots(40), 16);
    }

    #[test]
    fn a_shake_swings_both_ways_and_ends_where_it_started() {
        assert_eq!(shake(0.0), px(0.0));
        assert!(shake(1.0).abs() < px(0.001));
        assert!(shake(0.125) > px(6.0), "{:?}", shake(0.125));
        assert!(shake(0.375) < px(-4.0), "{:?}", shake(0.375));
        // Each swing is smaller than the one before.
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
