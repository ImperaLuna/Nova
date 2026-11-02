//! File contents to MIME types: the shared MIME database's binary `magic`
//! files. Each type has a priority and a tree of rules ("these bytes, at
//! one of these offsets"); a type matches when one rule at the top matches
//! and, if that rule has rules nested under it, one of those does too, all
//! the way down. Only the start of a file is read, as far as the furthest
//! rule looks.

const HEADER: &[u8] = b"MIME-Magic\0\n";
/// Rules never need more of a file than this, even an ISO image's (32 KiB in).
pub const MAX_READ: usize = 64 * 1024;

/// One rule: `value` (under `mask`) somewhere in `offset..offset + range`.
#[derive(Debug)]
struct Rule {
    indent: u32,
    offset: usize,
    range: usize,
    value: Vec<u8>,
    mask: Option<Vec<u8>>,
}

/// One type's rules.
#[derive(Debug)]
struct Section {
    priority: u32,
    mime: String,
    rules: Vec<Rule>,
}

/// Every data folder's rules, the highest priority first.
#[derive(Debug, Default)]
pub struct Magic {
    sections: Vec<Section>,
    /// How much of a file the rules look at.
    extent: usize,
}

impl Magic {
    /// Adds a folder's `magic` file. A file that isn't one adds nothing.
    pub fn add(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut rest = bytes.strip_prefix(HEADER).ok_or("not a magic file")?;
        while !rest.is_empty() {
            let section;
            (section, rest) = parse_section(rest)?;
            self.extent = self.extent.max(section.rules.iter().map(Rule::extent).max().unwrap_or(0)).min(MAX_READ);
            self.sections.push(section);
        }
        // Stable: within a priority, the order the folders gave.
        self.sections.sort_by(|a, b| b.priority.cmp(&a.priority));
        Ok(())
    }

    /// How many bytes from a file's start the rules need.
    pub fn extent(&self) -> usize {
        self.extent
    }

    /// The type `data` (a file's start) matches, the highest priority
    /// first, with that priority; among `only` when given.
    pub fn find(&self, data: &[u8], only: Option<&dyn Fn(&str) -> bool>) -> Option<(&str, u32)> {
        self.sections
            .iter()
            .filter(|section| only.is_none_or(|only| only(&section.mime)))
            .find(|section| matches_any(&section.rules, 0, data))
            .map(|section| (section.mime.as_str(), section.priority))
    }
}

impl Rule {
    fn extent(&self) -> usize {
        self.offset.saturating_add(self.range).saturating_add(self.value.len())
    }

    fn matches(&self, data: &[u8]) -> bool {
        (self.offset..self.offset.saturating_add(self.range)).any(|start| {
            let Some(window) = data.get(start..start + self.value.len()) else { return false };
            match &self.mask {
                None => window == self.value,
                Some(mask) => {
                    window.iter().zip(&self.value).zip(mask).all(|((byte, value), mask)| byte & mask == value & mask)
                }
            }
        })
    }
}

/// Whether one of the rules at `indent` in `rules` matches, with one of its
/// own nested rules when it has some.
fn matches_any(rules: &[Rule], indent: u32, data: &[u8]) -> bool {
    let mut i = 0;
    while i < rules.len() {
        let rule = &rules[i];
        // Its nested rules run until the next one at its own indent or less.
        let nested_end =
            rules[i + 1..].iter().position(|next| next.indent <= indent).map_or(rules.len(), |at| i + 1 + at);
        let nested = &rules[i + 1..nested_end];
        if rule.indent == indent && rule.matches(data) && (nested.is_empty() || matches_any(nested, indent + 1, data)) {
            return true;
        }
        i = nested_end;
    }
    false
}

/// A section, `[priority:type]\n` and its rules, and what follows it.
fn parse_section(bytes: &[u8]) -> Result<(Section, &[u8]), String> {
    let header_end = bytes.iter().position(|&byte| byte == b'\n').ok_or("a section header isn't ended")?;
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(|_| "a section header isn't text")?;
    let (priority, mime) = header
        .strip_prefix('[')
        .and_then(|header| header.strip_suffix(']'))
        .and_then(|header| header.split_once(':'))
        .ok_or_else(|| format!("a bad section header {header:?}"))?;
    let priority = priority.parse().map_err(|_| format!("a bad priority in {header:?}"))?;
    let mut section = Section { priority, mime: mime.to_string(), rules: Vec::new() };
    let mut rest = &bytes[header_end + 1..];
    while !rest.is_empty() && rest[0] != b'[' {
        match parse_rule(rest) {
            Ok((rule, after)) => {
                section.rules.push(rule);
                rest = after;
            }
            // A line this reader doesn't know (a later extension) is skipped.
            Err(Skip(at)) => rest = &rest[at..],
        }
    }
    Ok((section, rest))
}

/// Where the line a rule couldn't be read from ends.
struct Skip(usize);

/// One rule line: `[indent]>offset=<u16 length><value>[&<mask>][~size][+range]\n`.
fn parse_rule(bytes: &[u8]) -> Result<(Rule, &[u8]), Skip> {
    let line_end = |from: usize| {
        bytes[from.min(bytes.len())..].iter().position(|&byte| byte == b'\n').map_or(bytes.len(), |at| from + at + 1)
    };
    let skip = |from: usize| Skip(line_end(from));
    let mut at = 0;
    let indent = number(bytes, &mut at).unwrap_or(0);
    if bytes.get(at) != Some(&b'>') {
        return Err(skip(at));
    }
    at += 1;
    let offset = number(bytes, &mut at).ok_or_else(|| skip(at))?;
    if bytes.get(at) != Some(&b'=') {
        return Err(skip(at));
    }
    at += 1;
    let length = bytes
        .get(at..at + 2)
        .map(|pair| usize::from(u16::from_be_bytes([pair[0], pair[1]])))
        .ok_or_else(|| skip(at))?;
    at += 2;
    let mut value = bytes.get(at..at + length).ok_or(Skip(bytes.len()))?.to_vec();
    at += length;
    let mut mask = None;
    if bytes.get(at) == Some(&b'&') {
        at += 1;
        mask = Some(bytes.get(at..at + length).ok_or(Skip(bytes.len()))?.to_vec());
        at += length;
    }
    let mut word_size = 1;
    if bytes.get(at) == Some(&b'~') {
        at += 1;
        word_size = number(bytes, &mut at).ok_or_else(|| skip(at))?;
    }
    let mut range = 1;
    if bytes.get(at) == Some(&b'+') {
        at += 1;
        range = number(bytes, &mut at).ok_or_else(|| skip(at))?;
    }
    if bytes.get(at) != Some(&b'\n') {
        return Err(skip(at));
    }
    // Values of 2 and 4-byte words are big-endian in the file and compared
    // in the host's order.
    if cfg!(target_endian = "little") && matches!(word_size, 2 | 4) && length % word_size == 0 {
        for word in value.chunks_mut(word_size) {
            word.reverse();
        }
        if let Some(mask) = &mut mask {
            for word in mask.chunks_mut(word_size) {
                word.reverse();
            }
        }
    }
    let rule = Rule { indent: indent as u32, offset, range: range.max(1), value, mask };
    Ok((rule, &bytes[at + 1..]))
}

/// The decimal number at `at`, moving past it.
fn number(bytes: &[u8], at: &mut usize) -> Option<usize> {
    let digits = bytes[*at..].iter().take_while(|byte| byte.is_ascii_digit()).count();
    let text = std::str::from_utf8(&bytes[*at..*at + digits]).ok()?;
    let value = text.parse().ok()?;
    *at += digits;
    Some(value)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// A magic file's bytes, from sections of `(priority, type, rules)`,
    /// each rule `(indent, offset, value, mask, range)`.
    pub fn magic_file(sections: &[(u32, &str, &[(u32, usize, &[u8], Option<&[u8]>, usize)])]) -> Vec<u8> {
        let mut bytes = HEADER.to_vec();
        for (priority, mime, rules) in sections {
            bytes.extend(format!("[{priority}:{mime}]\n").as_bytes());
            for (indent, offset, value, mask, range) in *rules {
                if *indent > 0 {
                    bytes.extend(indent.to_string().as_bytes());
                }
                bytes.extend(format!(">{offset}=").as_bytes());
                bytes.extend((value.len() as u16).to_be_bytes());
                bytes.extend(*value);
                if let Some(mask) = mask {
                    bytes.push(b'&');
                    bytes.extend(*mask);
                }
                if *range > 1 {
                    bytes.extend(format!("+{range}").as_bytes());
                }
                bytes.push(b'\n');
            }
        }
        bytes
    }

    fn magic(bytes: &[u8]) -> Magic {
        let mut magic = Magic::default();
        magic.add(bytes).unwrap();
        magic
    }

    #[test]
    fn the_highest_priority_match_wins() {
        let magic = magic(&magic_file(&[
            (50, "application/zip", &[(0, 0, b"PK\x03\x04", None, 1)]),
            (
                80,
                "application/epub+zip",
                &[(0, 0, b"PK\x03\x04", None, 1), (1, 30, b"mimetypeapplication/epub+zip", None, 1)],
            ),
            (50, "application/pdf", &[(0, 0, b"%PDF-", None, 1024)]),
        ]));
        let epub = [b"PK\x03\x04".as_slice(), &[0; 26], b"mimetypeapplication/epub+zip"].concat();
        assert_eq!(magic.find(&epub, None), Some(("application/epub+zip", 80)));
        assert_eq!(magic.find(b"PK\x03\x04 plain zip", None), Some(("application/zip", 50)));
        assert_eq!(magic.find(b"junk %PDF-1.7", None), Some(("application/pdf", 50)), "found within its range");
        assert_eq!(magic.find(b"nothing", None), None);
        let only_zip = |mime: &str| mime == "application/zip";
        assert_eq!(magic.find(&epub, Some(&only_zip)), Some(("application/zip", 50)));
        assert!(magic.extent() >= 30 + 28);
    }

    #[test]
    fn masks_and_nested_rules_must_all_fit() {
        let magic = magic(&magic_file(&[(
            50,
            "image/x-test",
            &[(0, 0, b"AB", Some(b"\xff\xdf"), 1), (1, 2, b"1", None, 1), (1, 2, b"2", None, 1), (2, 3, b"!", None, 1)],
        )]));
        assert!(magic.find(b"AB1", None).is_some());
        assert!(magic.find(b"Ab1", None).is_some(), "the mask ignores the case bit");
        assert!(magic.find(b"AB2!", None).is_some());
        assert!(magic.find(b"AB2?", None).is_none());
        assert!(magic.find(b"AB3", None).is_none());
    }

    #[test]
    fn unknown_lines_are_skipped_and_bad_files_refused() {
        let mut bytes = magic_file(&[(50, "text/x-a", &[(0, 0, b"A", None, 1)])]);
        bytes.extend(b"[50:text/x-b]\n>0=\x00\x01B\n?some later extension\n>4=\x00\x01Z\n");
        let magic = magic(&bytes);
        assert_eq!(magic.find(b"B", None), Some(("text/x-b", 50)));
        assert_eq!(magic.find(b"....Z", None), Some(("text/x-b", 50)));
        assert!(Magic::default().add(b"not magic").is_err());
        assert!(Magic::default().add(b"MIME-Magic\0\n[50:x").is_err());
    }
}
