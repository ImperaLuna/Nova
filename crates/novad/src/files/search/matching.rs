//! How a search query matches a file name: the query is split into words,
//! and a name matches when it holds every word, case aside, anywhere in it
//! ("crim des" finds "Crimson Desert"). How well it matches ([`Fit`])
//! ranks the results, and where each word is ([`Query::marks`]) is what
//! the UI highlights.
//!
//! Most names are ASCII, so those compare byte by byte ignoring ASCII
//! case, without making a lowercase copy; other names are lowercased with
//! Unicode's rules first.

use nova_protocol::TextMark;

/// A parsed search: its words, lowercased, each once.
pub struct Query {
    words: Vec<String>,
    /// The words with one space between, which a whole name equals.
    whole: String,
    /// Every word is ASCII, so a name that isn't can still match, but an
    /// ASCII name can only match one that is.
    ascii: bool,
}

/// How well a name fits a query, best first; it ranks the results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fit {
    /// The name is the query, case aside.
    Exact,
    /// The name starts with the first word.
    Start,
    /// The first word starts a word in the name ("desert" in "Crimson
    /// Desert", "crimson_desert" or "CrimsonDesert").
    WordStart,
    /// The words are somewhere in the name.
    Inside,
}

impl Query {
    /// The words of `text`; `None` when it has none.
    pub fn parse(text: &str) -> Option<Self> {
        let mut words: Vec<String> = Vec::new();
        for word in text.split_whitespace().map(fold) {
            if !words.contains(&word) {
                words.push(word);
            }
        }
        let ascii = words.iter().all(|word| word.is_ascii());
        let whole = words.join(" ");
        (!words.is_empty()).then_some(Self { words, whole, ascii })
    }

    /// How well `name` fits, or `None` when it lacks a word. `ascii` says
    /// whether `name` is ASCII, which the index keeps for each name.
    pub fn fit(&self, name: &str, ascii: bool) -> Option<Fit> {
        if ascii {
            if !self.ascii {
                return None;
            }
            let first = ascii_find(name.as_bytes(), self.words[0].as_bytes())?;
            if !self.words[1..].iter().all(|word| ascii_find(name.as_bytes(), word.as_bytes()).is_some()) {
                return None;
            }
            return Some(self.rank(name, first, || name.eq_ignore_ascii_case(&self.whole)));
        }
        let folded = Folded::of(name);
        let first = folded.find(&self.words[0])?;
        if !self.words[1..].iter().all(|word| folded.find(word).is_some()) {
            return None;
        }
        Some(self.rank(name, first.0, || folded.lower == self.whole))
    }

    /// Where each word first is in `name`, in UTF-16 code units (what QML's
    /// strings count), in order and without overlaps.
    pub fn marks(&self, name: &str) -> Vec<TextMark> {
        let folded = Folded::of(name);
        let mut found: Vec<(usize, usize)> = self.words.iter().filter_map(|word| folded.find(word)).collect();
        found.sort_unstable();
        let mut marks: Vec<TextMark> = Vec::new();
        let mut covered = 0;
        for (start, end) in found {
            let start = start.max(covered);
            if start >= end {
                continue;
            }
            covered = end;
            let units = |bytes: &str| u32::try_from(bytes.encode_utf16().count()).unwrap_or(u32::MAX);
            marks.push(TextMark { start: units(&name[..start]), length: units(&name[start..end]) });
        }
        marks
    }

    /// The fit of a name holding every word, the first at byte `first`;
    /// `whole` says whether the name is the query.
    fn rank(&self, name: &str, first: usize, whole: impl FnOnce() -> bool) -> Fit {
        if first == 0 && whole() {
            Fit::Exact
        } else if first == 0 {
            Fit::Start
        } else if starts_word(name, first) {
            Fit::WordStart
        } else {
            Fit::Inside
        }
    }
}

/// `text` lowercased a character at a time, the same way names are
/// ([`Folded`]), so a final sigma folds like any other.
fn fold(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// Whether the character at byte `at` of `name` starts a word: it follows
/// something that isn't a letter or digit, or it's a capital after a
/// lowercase letter.
fn starts_word(name: &str, at: usize) -> bool {
    let (Some(before), Some(here)) = (name[..at].chars().next_back(), name[at..].chars().next()) else {
        return true;
    };
    !before.is_alphanumeric() || (before.is_lowercase() && here.is_uppercase())
}

/// Where `needle` (lowercase ASCII) first is in `haystack`, ASCII case
/// aside, as a byte offset.
fn ascii_find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let (&first, rest) = needle.split_first()?;
    let last_start = haystack.len().checked_sub(needle.len())?;
    (0..=last_start).find(|&at| {
        haystack[at].to_ascii_lowercase() == first && haystack[at + 1..at + needle.len()].eq_ignore_ascii_case(rest)
    })
}

/// A name lowercased, and where each of its bytes came from in the name.
struct Folded<'a> {
    name: &'a str,
    lower: String,
    /// For each byte of `lower`, the byte of `name` its character starts at.
    from: Vec<usize>,
}

impl<'a> Folded<'a> {
    fn of(name: &'a str) -> Self {
        let mut lower = String::with_capacity(name.len());
        let mut from = Vec::with_capacity(name.len());
        for (at, character) in name.char_indices() {
            for folded in character.to_lowercase() {
                lower.push(folded);
                from.resize(lower.len(), at);
            }
        }
        Self { name, lower, from }
    }

    /// Where `word` (lowercase) first is, as a byte range of the name.
    fn find(&self, word: &str) -> Option<(usize, usize)> {
        let start = self.lower.find(word)?;
        let end = start + word.len();
        let name_end = self.from.get(end).copied().unwrap_or(self.name.len());
        Some((self.from[start], name_end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fit(query: &str, name: &str) -> Option<Fit> {
        Query::parse(query).unwrap().fit(name, name.is_ascii())
    }

    fn marks(query: &str, name: &str) -> Vec<(u32, u32)> {
        Query::parse(query).unwrap().marks(name).into_iter().map(|mark| (mark.start, mark.length)).collect()
    }

    #[test]
    fn every_word_must_be_in_the_name_case_aside() {
        assert_eq!(fit("crimson", "Crimson Desert"), Some(Fit::Start));
        assert_eq!(fit("DES crim", "Crimson Desert"), Some(Fit::WordStart));
        assert_eq!(fit("crim xyz", "Crimson Desert"), None);
        assert_eq!(fit("rims", "CRIMSON"), Some(Fit::Inside));
        assert!(Query::parse("  \t ").is_none());
    }

    #[test]
    fn the_fit_ranks_whole_names_then_starts_then_words() {
        assert_eq!(fit("steam", "Steam"), Some(Fit::Exact));
        assert_eq!(fit("crimson  DESERT", "Crimson Desert"), Some(Fit::Exact));
        assert_eq!(fit("crimson desert", "Crimson Desert notes.txt"), Some(Fit::Start));
        assert_eq!(fit("steam", "steamapps"), Some(Fit::Start));
        assert_eq!(fit("desert", "crimson_desert.exe"), Some(Fit::WordStart));
        assert_eq!(fit("desert", "CrimsonDesert.exe"), Some(Fit::WordStart));
        assert_eq!(fit("esert", "CrimsonDesert.exe"), Some(Fit::Inside));
        assert!(Fit::Exact < Fit::Start && Fit::Start < Fit::WordStart && Fit::WordStart < Fit::Inside);
    }

    #[test]
    fn names_beyond_ascii_match_with_unicodes_case_rules() {
        assert_eq!(fit("öl", "Ölgemälde.png"), Some(Fit::Start));
        assert_eq!(fit("GEMÄLDE", "Ölgemälde.png"), Some(Fit::Inside));
        assert_eq!(fit("ölgemälde", "ÖLGEMÄLDE"), Some(Fit::Exact));
        // A final sigma folds the same in the query and the name.
        assert_eq!(fit("ΚΟΣΜΟΣ", "ΚΟΣΜΟΣ.txt"), Some(Fit::Start));
        // An ASCII name holds no other letters.
        assert_eq!(fit("ö", "Oil.png"), None);
    }

    #[test]
    fn marks_count_utf16_units_and_never_overlap() {
        assert_eq!(marks("desert crimson", "Crimson Desert"), vec![(0, 7), (8, 6)]);
        // "🎮" is two UTF-16 units, "é" one.
        assert_eq!(marks("café", "🎮 Café"), vec![(3, 4)]);
        assert_eq!(marks("crimson rims", "Crimson"), vec![(0, 7)]);
    }
}
