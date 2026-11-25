use nova_look::motion::Eased;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub struct Digit {
    pub shown: char,
    pub leaving: Option<char>,
    pub roll: Eased,
}

impl Digit {
    fn still(shown: char, now: Instant) -> Self {
        Self { shown, leaving: None, roll: Eased::at(1.0, now) }
    }

    pub fn rolls(&self) -> bool {
        self.shown.is_ascii_digit()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Digits {
    list: Vec<Digit>,
}

impl Digits {
    pub fn new(text: &str, now: Instant) -> Self {
        Self { list: text.chars().map(|shown| Digit::still(shown, now)).collect() }
    }

    pub fn set(&mut self, text: &str, now: Instant) {
        if text.chars().count() != self.list.len() {
            *self = Self::new(text, now);
            return;
        }
        for (digit, shown) in self.list.iter_mut().zip(text.chars()) {
            if digit.shown == shown {
                continue;
            }
            let mut roll = Eased::at(0.0, now);
            roll.set_target(1.0, now);
            *digit = Digit { shown, leaving: Some(digit.shown), roll };
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Digit> {
        self.list.iter()
    }

    pub fn rolling(&self, now: Instant) -> bool {
        self.list.iter().any(|digit| !digit.roll.settled(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_look::tokens::DURATION_PANEL;

    #[test]
    fn only_the_digits_that_changed_roll() {
        let start = Instant::now();
        let mut digits = Digits::new("16:39", start);
        assert!(!digits.rolling(start));
        digits.set("16:40", start);
        let rolling: Vec<_> = digits.iter().map(|digit| digit.leaving).collect();
        assert_eq!(rolling, [None, None, None, Some('3'), Some('9')]);
        assert!(digits.rolling(start));
        assert!(!digits.rolling(start + DURATION_PANEL));
        let shown: String = digits.iter().map(|digit| digit.shown).collect();
        assert_eq!(shown, "16:40");
    }

    #[test]
    fn a_roll_runs_from_the_old_digit_to_the_new_on_the_panel_curve() {
        let start = Instant::now();
        let mut digits = Digits::new("09:59", start);
        digits.set("10:00", start);
        let first = digits.iter().next().cloned().unwrap();
        assert_eq!(first.roll.value(start), 0.0);
        let halfway = first.roll.value(start + DURATION_PANEL / 2);
        assert!(halfway > 0.5 && halfway < 1.0, "{halfway}");
        assert_eq!(first.roll.value(start + DURATION_PANEL), 1.0);
    }

    #[test]
    fn the_colon_never_rolls() {
        let digits = Digits::new("12:00", Instant::now());
        let rolls: Vec<_> = digits.iter().map(Digit::rolls).collect();
        assert_eq!(rolls, [true, true, false, true, true]);
    }

    #[test]
    fn a_text_of_another_length_shows_at_once() {
        let start = Instant::now();
        let mut digits = Digits::new("9:59", start);
        digits.set("10:00", start);
        assert!(!digits.rolling(start));
    }
}
