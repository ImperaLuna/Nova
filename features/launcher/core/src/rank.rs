//! Ranking apps for a launcher search. A match in the name counts most (its
//! start, then a word's start, then anywhere, then its letters in order),
//! then the generic name, keywords and program, then the comment. Apps the
//! user opens often climb a little; ties go to the shorter name.

use crate::apps::desktop_entry::Entry;
use nova_protocol::TextRun;

/// What a match in the name is worth, by where it starts.
const NAME_START: u32 = 1000;
const NAME_WORD: u32 = 800;
const NAME_ANYWHERE: u32 = 600;
/// The query's letters in order, gaps allowed (`ffx` in Firefox).
const NAME_LETTERS: u32 = 300;
/// A generic name's, keyword's or the program's start or word start, and
/// anywhere in them.
const DETAIL_WORD: u32 = 250;
const DETAIL_ANYWHERE: u32 = 200;
const COMMENT: u32 = 100;
/// Each launch adds this much, up to the most: enough to reorder close
/// matches, never to beat a better kind of match.
const PER_LAUNCH: u32 = 10;
const MOST_FOR_LAUNCHES: u32 = 150;

/// How well `query` (lower case, trimmed, not empty) matches `entry`, and
/// which characters of its name it matched; `None` when it doesn't.
/// Several words must each match somewhere, and the weakest decides.
pub fn score(entry: &Entry, query: &str, launches: u32) -> Option<(u32, Vec<TextRun>)> {
    let name = entry.name.to_lowercase();
    let program = entry.exec.first().map(|program| program.rsplit('/').next().unwrap_or(program).to_lowercase());
    let details: Vec<String> =
        entry.generic_name.iter().chain(&entry.keywords).map(|detail| detail.to_lowercase()).chain(program).collect();
    let comment = entry.comment.as_deref().map(str::to_lowercase);

    let mut weakest = u32::MAX;
    let mut marked = vec![false; name.chars().count()];
    for word in query.split_whitespace() {
        let in_name = name_match(&name, word);
        let best = [
            in_name.as_ref().map(|(score, _)| *score),
            details.iter().filter_map(|detail| detail_match(detail, word)).max(),
            comment.as_deref().filter(|comment| comment.contains(word)).map(|_| COMMENT),
        ]
        .into_iter()
        .flatten()
        .max()?;
        weakest = weakest.min(best);
        if let Some((_, at)) = in_name {
            for index in at {
                if let Some(mark) = marked.get_mut(index) {
                    *mark = true;
                }
            }
        }
    }
    (weakest != u32::MAX)
        .then(|| (weakest + (launches * PER_LAUNCH).min(MOST_FOR_LAUNCHES), runs(&entry.name, &marked)))
}

/// A match in the (lower case) name: its score and the characters it covers.
fn name_match(name: &str, word: &str) -> Option<(u32, Vec<usize>)> {
    let chars: Vec<char> = name.chars().collect();
    let wanted: Vec<char> = word.chars().collect();
    let span = |start: usize| (start..start + wanted.len()).collect();
    let found: Vec<usize> = (0..chars.len()).filter(|&at| chars[at..].starts_with(&wanted)).collect();
    if found.first() == Some(&0) {
        return Some((NAME_START, span(0)));
    }
    if let Some(&at) = found.iter().find(|&&at| word_starts_at(&chars, at)) {
        return Some((NAME_WORD, span(at)));
    }
    if let Some(&at) = found.first() {
        return Some((NAME_ANYWHERE, span(at)));
    }
    let mut at = Vec::with_capacity(wanted.len());
    let mut next = wanted.iter().peekable();
    for (index, c) in chars.iter().enumerate() {
        if next.peek() == Some(&c) {
            at.push(index);
            next.next();
        }
    }
    next.peek().is_none().then_some((NAME_LETTERS, at))
}

fn detail_match(detail: &str, word: &str) -> Option<u32> {
    let chars: Vec<char> = detail.chars().collect();
    let wanted: Vec<char> = word.chars().collect();
    let found: Vec<usize> = (0..chars.len()).filter(|&at| chars[at..].starts_with(&wanted)).collect();
    if found.iter().any(|&at| word_starts_at(&chars, at)) {
        Some(DETAIL_WORD)
    } else {
        (!found.is_empty()).then_some(DETAIL_ANYWHERE)
    }
}

fn word_starts_at(chars: &[char], at: usize) -> bool {
    at == 0 || !chars[at - 1].is_alphanumeric()
}

/// `name` cut into runs, `matched` where `marked` (one per character) says.
fn runs(name: &str, marked: &[bool]) -> Vec<TextRun> {
    let mut runs: Vec<TextRun> = Vec::new();
    // Lower-casing can change how many characters there are (rare: İ); then
    // nothing is marked rather than the wrong letters.
    let fits = marked.len() == name.chars().count();
    for (index, c) in name.chars().enumerate() {
        let matched = fits && marked[index];
        match runs.last_mut() {
            Some(run) if run.matched == matched => run.text.push(c),
            _ => runs.push(TextRun { text: c.to_string(), matched }),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> Entry {
        Entry {
            id: format!("{}.desktop", name.to_lowercase().replace(' ', "-")),
            name: name.into(),
            generic_name: None,
            comment: None,
            icon: None,
            keywords: vec![],
            exec: vec![name.to_lowercase()],
            terminal: false,
            path: None,
            mime_types: vec![],
            listed: true,
            window_class: None,
            categories: vec![],
        }
    }

    fn marked(runs: &[TextRun]) -> String {
        runs.iter().map(|run| if run.matched { format!("[{}]", run.text) } else { run.text.clone() }).collect()
    }

    fn rank(apps: &[Entry], query: &str) -> Vec<String> {
        let mut hits: Vec<(u32, &Entry)> =
            apps.iter().filter_map(|app| score(app, query, 0).map(|(score, _)| (score, app))).collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.len().cmp(&b.1.name.len())));
        hits.into_iter().map(|(_, app)| app.name.clone()).collect()
    }

    #[test]
    fn name_starts_beat_word_starts_beat_anywhere_beat_letters() {
        let apps = [app("Firefox"), app("Thunderbird Mail"), app("GNU Image Manipulation Program"), app("Signal")];
        assert_eq!(rank(&apps, "fi"), ["Firefox"]);
        assert_eq!(rank(&apps, "ma"), ["Thunderbird Mail", "GNU Image Manipulation Program"]);
        assert_eq!(rank(&apps, "ig"), ["Signal", "GNU Image Manipulation Program"]);
        assert_eq!(rank(&apps, "ffx"), ["Firefox"]);
        assert!(rank(&apps, "zzz").is_empty());
    }

    #[test]
    fn matched_letters_are_marked() {
        let firefox = app("Firefox");
        assert_eq!(marked(&score(&firefox, "fire", 0).unwrap().1), "[Fire]fox");
        assert_eq!(marked(&score(&firefox, "ffx", 0).unwrap().1), "[F]ire[f]o[x]");
        let files = Entry { generic_name: Some("File Manager".into()), ..app("Nautilus") };
        assert_eq!(marked(&score(&files, "manager", 0).unwrap().1), "Nautilus", "the generic name matched");
        assert_eq!(marked(&score(&app("Visual Studio Code"), "vis code", 0).unwrap().1), "[Vis]ual Studio [Code]");
    }

    #[test]
    fn details_count_less_than_the_name() {
        let files =
            Entry { generic_name: Some("File Manager".into()), keywords: vec!["folder".into()], ..app("Nautilus") };
        let filezilla = app("FileZilla");
        assert_eq!(rank(&[files.clone(), filezilla], "file"), ["FileZilla", "Nautilus"]);
        assert_eq!(rank(std::slice::from_ref(&files), "fold"), ["Nautilus"]);
        assert_eq!(rank(&[files], "nautilus"), ["Nautilus"]);
        let commented = Entry { comment: Some("Chat with your friends".into()), ..app("Signal") };
        assert_eq!(score(&commented, "friends", 0).unwrap().0, COMMENT);
        // The program, for apps whose name says something else.
        let code = Entry { exec: vec!["/usr/bin/codium".into(), "--new".into()], ..app("VSCodium") };
        assert_eq!(score(&code, "codium", 0).unwrap().0, NAME_ANYWHERE);
        assert_eq!(score(&Entry { name: "Editor".into(), ..code }, "codium", 0).unwrap().0, DETAIL_WORD);
    }

    #[test]
    fn every_word_must_match() {
        let apps = [app("Visual Studio Code"), app("Code Blocks")];
        assert_eq!(rank(&apps, "code vis"), ["Visual Studio Code"]);
        assert_eq!(rank(&apps, "code"), ["Code Blocks", "Visual Studio Code"]);
    }

    #[test]
    fn launches_reorder_close_matches_only() {
        let (often, rarely) = (app("Foot"), app("Fooview"));
        assert!(score(&often, "foo", 3).unwrap().0 > score(&rarely, "foo", 0).unwrap().0);
        // Opened a thousand times, a letters-only match still trails a start.
        assert!(
            score(&app("Gnome Office Online"), "goo", 1000).unwrap().0 < score(&app("Google"), "goo", 0).unwrap().0
        );
    }
}
