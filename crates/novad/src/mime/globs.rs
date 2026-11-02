//! File names to MIME types: the shared MIME database's `globs2` files
//! (`weight:type:pattern[:flags]`), merged across the data folders.
//!
//! Patterns come in three kinds, looked up as the spec says: a literal name
//! (`makefile`) first, then suffixes (`*.tar.gz`) and other globs
//! (`*.so.[0-9]*`) together, the highest weight winning and, among those,
//! the longest pattern. Most patterns are `*.<extension>`, kept in a map by
//! their last extension, so a lookup is a hash lookup plus a short scan,
//! with no allocation: a folder of 100,000 files is typed this way.
//! Patterns match without regard to ASCII case unless flagged `cs`.

use std::collections::{HashMap, HashSet};

/// The flag that makes a pattern case-sensitive.
const CASE_SENSITIVE: &str = "cs";
/// A type listed with this pattern drops the patterns lower folders gave it.
const NO_GLOBS: &str = "__NOGLOBS__";

/// One pattern and the type it names.
#[derive(Debug, Clone)]
struct Glob {
    weight: u32,
    mime: String,
    pattern: String,
    /// The pattern's characters, for matching it as a shell glob.
    chars: Vec<char>,
    case_sensitive: bool,
    /// Which `add` gave it, counting up: a later folder outranks an
    /// earlier one where their patterns tie.
    folder: u32,
}

/// The patterns of every data folder.
#[derive(Debug, Default)]
pub struct Globs {
    /// Whole names, lower case, to their patterns.
    literals: HashMap<String, Vec<Glob>>,
    /// `*.<suffix>` patterns, by the suffix's last extension in lower case
    /// (`gz` for `*.tar.gz`).
    extensions: HashMap<String, Vec<Glob>>,
    /// Every other pattern (`*~`, `*.so.[0-9]*`, `sconscript.*`).
    others: Vec<Glob>,
    /// How many folders were added.
    folders: u32,
}

/// What a name matched: the best weight's types, the longest pattern first.
pub struct Matches<'a> {
    pub types: Vec<&'a str>,
}

impl Globs {
    /// Adds a folder's `globs2` text. Folders come in from the least
    /// important to the most, so a folder's `__NOGLOBS__` drops what came
    /// before it.
    pub fn add(&mut self, text: &str) {
        self.folders += 1;
        let folder = self.folders;
        let mut cleared = HashSet::new();
        for line in text.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let mut fields = line.splitn(4, ':');
            let (Some(weight), Some(mime), Some(pattern)) = (fields.next(), fields.next(), fields.next()) else {
                continue;
            };
            let Ok(weight) = weight.parse() else { continue };
            if pattern == NO_GLOBS {
                if cleared.insert(mime.to_string()) {
                    self.remove(mime);
                }
                continue;
            }
            let case_sensitive = fields.next().is_some_and(|flags| flags.split(',').any(|flag| flag == CASE_SENSITIVE));
            let chars = pattern.chars().collect();
            self.insert(Glob {
                weight,
                mime: mime.to_string(),
                pattern: pattern.to_string(),
                chars,
                case_sensitive,
                folder,
            });
        }
    }

    fn insert(&mut self, glob: Glob) {
        if !has_wildcard(&glob.pattern) {
            self.literals.entry(glob.pattern.to_ascii_lowercase()).or_default().push(glob);
        } else if let Some(suffix) = glob.pattern.strip_prefix('*')
            && !has_wildcard(suffix)
            && let Some((_, extension)) = suffix.rsplit_once('.')
        {
            self.extensions.entry(extension.to_ascii_lowercase()).or_default().push(glob);
        } else {
            self.others.push(glob);
        }
    }

    fn remove(&mut self, mime: &str) {
        for globs in self.literals.values_mut().chain(self.extensions.values_mut()) {
            globs.retain(|glob| glob.mime != mime);
        }
        self.others.retain(|glob| glob.mime != mime);
    }

    /// The types `name` (a file name, not a path) matches: a literal name
    /// decides alone; otherwise the best weight wins, then the longest
    /// pattern. Empty when nothing matches; several when that's a tie.
    pub fn find(&self, name: &str) -> Matches<'_> {
        let literal: Vec<&Glob> = lookup(&self.literals, name)
            .into_iter()
            .flatten()
            .filter(|glob| !glob.case_sensitive || glob.pattern == name)
            .collect();
        if !literal.is_empty() {
            return best(literal);
        }
        let by_extension = name
            .rsplit_once('.')
            .and_then(|(_, extension)| lookup(&self.extensions, extension))
            .into_iter()
            .flatten()
            .filter(|glob| ends_with(name, &glob.pattern[1..], glob.case_sensitive));
        // The name's characters, once for all the shell globs.
        let name_chars: Vec<char> = if self.others.is_empty() { Vec::new() } else { name.chars().collect() };
        let others = self.others.iter().filter(|glob| matches(&glob.chars, &name_chars, glob.case_sensitive));
        best(by_extension.chain(others).collect())
    }
}

/// `map`'s entry for `key` in lower case; only a key with upper case in it
/// is copied to look it up.
fn lookup<'a>(map: &'a HashMap<String, Vec<Glob>>, key: &str) -> Option<&'a Vec<Glob>> {
    if key.bytes().any(|byte| byte.is_ascii_uppercase()) { map.get(&key.to_ascii_lowercase()) } else { map.get(key) }
}

/// The types of the best weight, then the longest pattern; among those,
/// a more important folder's first.
fn best(found: Vec<&Glob>) -> Matches<'_> {
    let Some(weight) = found.iter().map(|glob| glob.weight).max() else { return Matches { types: vec![] } };
    let heaviest = found.iter().filter(|glob| glob.weight == weight);
    let longest = heaviest.clone().map(|glob| glob.pattern.len()).max().unwrap_or(0);
    let mut tied: Vec<&Glob> = heaviest.filter(|glob| glob.pattern.len() == longest).copied().collect();
    // Stable: within a folder, the order it listed them.
    tied.sort_by_key(|glob| std::cmp::Reverse(glob.folder));
    let mut types: Vec<&str> = Vec::new();
    for glob in tied {
        if !types.contains(&glob.mime.as_str()) {
            types.push(&glob.mime);
        }
    }
    Matches { types }
}

fn has_wildcard(pattern: &str) -> bool {
    pattern.contains(['*', '?', '['])
}

fn ends_with(name: &str, suffix: &str, case_sensitive: bool) -> bool {
    name.len() >= suffix.len()
        && name.is_char_boundary(name.len() - suffix.len())
        && if case_sensitive {
            name.ends_with(suffix)
        } else {
            name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
        }
}

/// Whether `name` matches the shell glob `pattern` (`*`, `?`, `[...]`,
/// `[!...]`), as fnmatch does without flags.
fn matches(pattern: &[char], name: &[char], case_sensitive: bool) -> bool {
    let same = |a: char, b: char| if case_sensitive { a == b } else { a.eq_ignore_ascii_case(&b) };
    // Where to go back to when a later part fails: just after the last `*`,
    // with it swallowing one more character.
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        let step = match pattern.get(p) {
            Some('*') => {
                star = Some((p + 1, n));
                p += 1;
                continue;
            }
            Some('?') => Some(p + 1),
            Some('[') => class(pattern, p, name[n], case_sensitive),
            Some(&c) if same(c, name[n]) => Some(p + 1),
            _ => None,
        };
        match (step, star) {
            (Some(next), _) => {
                p = next;
                n += 1;
            }
            (None, Some((after, start))) => {
                p = after;
                n = start + 1;
                star = Some((after, start + 1));
            }
            (None, None) => return false,
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// Where the pattern goes on after the class opening at `open`, if `c` is
/// in it.
fn class(pattern: &[char], open: usize, c: char, case_sensitive: bool) -> Option<usize> {
    // The forms of `c` that count: any ASCII case unless the pattern is
    // case-sensitive.
    let forms = if case_sensitive { [c; 3] } else { [c, c.to_ascii_lowercase(), c.to_ascii_uppercase()] };
    let mut i = open + 1;
    let negated = matches!(pattern.get(i), Some('!' | '^'));
    if negated {
        i += 1;
    }
    let mut found = false;
    let mut first = true;
    loop {
        let &start = pattern.get(i)?;
        if start == ']' && !first {
            break;
        }
        first = false;
        if pattern.get(i + 1) == Some(&'-') && pattern.get(i + 2).is_some_and(|&end| end != ']') {
            let end = pattern[i + 2];
            found |= forms.iter().any(|form| (start..=end).contains(form));
            i += 3;
        } else {
            found |= forms.contains(&start);
            i += 1;
        }
    }
    (found != negated).then_some(i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn globs(text: &str) -> Globs {
        let mut globs = Globs::default();
        globs.add(text);
        globs
    }

    fn find<'a>(globs: &'a Globs, name: &str) -> Vec<&'a str> {
        globs.find(name).types
    }

    #[test]
    fn the_heaviest_then_longest_pattern_wins() {
        let globs = globs(
            "# comment\n50:application/gzip:*.gz\n50:application/x-compressed-tar:*.tar.gz\n\
             60:text/x-python:*.py\n50:text/plain:*.txt\n50:text/x-makefile:makefile\n\
             60:application/x-sharedlib:*.so.[0-9]*\n50:application/x-trash:*~\n",
        );
        assert_eq!(find(&globs, "a.tar.gz"), ["application/x-compressed-tar"]);
        assert_eq!(find(&globs, "a.gz"), ["application/gzip"]);
        assert_eq!(find(&globs, "Script.PY"), ["text/x-python"]);
        assert_eq!(find(&globs, "Makefile"), ["text/x-makefile"]);
        assert_eq!(find(&globs, "libfoo.so.6"), ["application/x-sharedlib"]);
        assert_eq!(find(&globs, "notes.txt~"), ["application/x-trash"]);
        assert!(find(&globs, "README").is_empty());
        assert!(find(&globs, ".gz-not").is_empty());
    }

    #[test]
    fn case_sensitive_patterns_need_the_exact_case() {
        let globs = globs("50:text/x-csrc:*.c:cs\n50:text/x-c++src:*.C:cs\n50:application/x-core:core:cs\n");
        assert_eq!(find(&globs, "main.c"), ["text/x-csrc"]);
        assert_eq!(find(&globs, "main.C"), ["text/x-c++src"]);
        assert_eq!(find(&globs, "core"), ["application/x-core"]);
        assert!(find(&globs, "CORE").is_empty());
    }

    #[test]
    fn ties_keep_every_type_for_magic_to_decide() {
        let globs = globs("50:video/mp4:*.mp4\n50:audio/mp4:*.mp4\n50:video/mp4:*.MP4:cs\n");
        assert_eq!(find(&globs, "clip.mp4"), ["video/mp4", "audio/mp4"]);
    }

    #[test]
    fn a_more_important_folder_can_drop_a_types_patterns() {
        let mut globs = globs("50:text/x-old:*.old\n50:text/plain:*.txt\n");
        globs.add("50:text/x-old:__NOGLOBS__\n50:text/x-old:*.older\n");
        assert!(find(&globs, "a.old").is_empty());
        assert_eq!(find(&globs, "a.older"), ["text/x-old"]);
        assert_eq!(find(&globs, "a.txt"), ["text/plain"]);
    }

    #[test]
    fn a_more_important_folder_wins_a_tie() {
        let mut globs = globs("50:application/x-system:*.foo\n");
        globs.add("50:application/x-user:*.foo\n");
        assert_eq!(find(&globs, "a.foo"), ["application/x-user", "application/x-system"]);
    }

    fn matches(pattern: &str, name: &str, case_sensitive: bool) -> bool {
        let chars = |text: &str| text.chars().collect::<Vec<_>>();
        super::matches(&chars(pattern), &chars(name), case_sensitive)
    }

    #[test]
    fn shell_globs_match_like_fnmatch() {
        assert!(matches("*.anim[1-9j]", "x.anim7", false));
        assert!(matches("*.anim[1-9j]", "x.animj", false));
        assert!(!matches("*.anim[1-9j]", "x.anim0", false));
        assert!(matches("sconscript.*", "SConscript.py", false));
        assert!(matches("a?c", "abc", true) && !matches("a?c", "ac", true));
        assert!(matches("[!a]x", "bx", true) && !matches("[!a]x", "ax", true));
        assert!(matches("*a*b*", "xxaxxbxx", true) && !matches("*a*b", "xxbxxa", true));
        assert!(!matches("[abc", "a", true), "an unclosed class matches nothing");
        assert!(!matches("*.[a-z]x", "file.Ax", true), "a case-sensitive range keeps its case");
        assert!(matches("*.[A-Z]", "file.q", false) && matches("*.[a-z]", "FILE.Q", false));
        assert!(matches("a?c", "aéc", true), "? is one character, not one byte");
    }
}
