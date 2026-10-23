//! The Quickshell versions Nova is tested with, from
//! `packaging/quickshell.toml` (compiled in, so a binary always checks
//! against the range it was built and tested for).
//!
//! Nova doesn't pin one exact Quickshell everywhere: distros ship their own,
//! built against their own Qt. Instead it checks the one it finds against a
//! tested range and says so when it's outside.

use serde::Deserialize;
use std::cmp::Ordering;
use std::fmt;

const RANGE_FILE: &str = include_str!("../../../packaging/quickshell.toml");
/// In `max`, matches any number in that place.
const WILDCARD: &str = "x";

/// A release number like `0.3.2`. Missing places count as 0 (`0.4` is
/// `0.4.0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version([u32; 3]);

impl Version {
    /// Reads `0.3.2`. In a bound, `x` (`0.3.x`) reads as higher than any
    /// number, so every release with that prefix is at or below it.
    fn parse(text: &str, allow_wildcard: bool) -> Option<Self> {
        let mut parts = [0; 3];
        let mut places = text.split('.');
        for part in &mut parts {
            *part = match places.next() {
                None => break,
                Some(WILDCARD) if allow_wildcard => u32::MAX,
                Some(place) => place.parse().ok()?,
            };
        }
        places.next().is_none().then_some(Self(parts))
    }
}

/// The supported range as written in the file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeFile {
    min: String,
    max: String,
}

#[derive(Debug)]
pub struct Range {
    file: RangeFile,
    min: Version,
    max: Version,
}

impl Range {
    /// The range Nova is built with.
    pub fn supported() -> Self {
        // A test reads this same file, so a bad edit fails the build's tests.
        Self::parse(RANGE_FILE).expect("packaging/quickshell.toml is checked by this crate's tests")
    }

    fn parse(text: &str) -> Result<Self, String> {
        let file: RangeFile = toml::from_str(text).map_err(|e| e.to_string())?;
        let min = Version::parse(&file.min, false).ok_or_else(|| format!("bad min version {:?}", file.min))?;
        let max = Version::parse(&file.max, true).ok_or_else(|| format!("bad max version {:?}", file.max))?;
        if min > max {
            return Err(format!("min {} is above max {}", file.min, file.max));
        }
        Ok(Self { file, min, max })
    }

    /// Why `version` (as `quickshell --version` prints it, `0.3.2`) isn't
    /// supported, ready to show; `None` when it's in the range.
    pub fn problem(&self, version: &str) -> Option<String> {
        let Some(found) = Version::parse(version, false) else {
            return Some(format!("Quickshell reports version {version:?}, which Nova can't check against {self}"));
        };
        let direction = match (found.cmp(&self.min), found.cmp(&self.max)) {
            (Ordering::Less, _) => "older",
            (_, Ordering::Greater) => "newer",
            _ => return None,
        };
        Some(format!(
            "Quickshell {version} is {direction} than Nova is tested with ({self}), so parts of the UI may break"
        ))
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} to {}", self.file.min, self.file.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(min: &str, max: &str) -> Range {
        Range::parse(&format!("min = {min:?}\nmax = {max:?}")).unwrap()
    }

    #[test]
    fn the_shipped_range_parses() {
        Range::parse(RANGE_FILE).unwrap();
    }

    #[test]
    fn a_wildcard_max_takes_every_patch_release() {
        let range = range("0.3.2", "0.3.x");
        for version in ["0.3.2", "0.3.3", "0.3.40"] {
            assert_eq!(range.problem(version), None, "{version}");
        }
        assert!(range.problem("0.3.1").unwrap().contains("older"));
        assert!(range.problem("0.4.0").unwrap().contains("newer"));
        assert!(range.problem("1.0").unwrap().contains("newer"));
    }

    #[test]
    fn an_exact_max_is_inclusive() {
        let range = range("0.2", "0.3.2");
        assert_eq!(range.problem("0.2.0"), None);
        assert_eq!(range.problem("0.3.2"), None);
        assert!(range.problem("0.3.3").unwrap().contains("newer"));
    }

    #[test]
    fn problems_name_the_range() {
        let message = range("0.3.2", "0.3.x").problem("0.4.0").unwrap();
        assert_eq!(
            message,
            "Quickshell 0.4.0 is newer than Nova is tested with (0.3.2 to 0.3.x), so parts of the UI may break"
        );
    }

    #[test]
    fn unreadable_versions_are_a_problem_not_a_pass() {
        let range = range("0.3.2", "0.3.x");
        for version in ["", "0.3.x", "0.3.2-git", "0.3.2.1", "v0.3.2"] {
            assert!(range.problem(version).unwrap().contains("can't check"), "{version:?}");
        }
    }

    #[test]
    fn bad_range_files_are_refused() {
        assert!(Range::parse("min = \"0.4\"\nmax = \"0.3.x\"").is_err());
        assert!(Range::parse("min = \"0.3.x\"\nmax = \"0.3.x\"").is_err());
        assert!(Range::parse("min = \"0.3\"\nmax = \"0.3\"\npin = \"0.3\"").is_err());
        assert!(Range::parse("min = \"0.3\"").is_err());
    }
}
