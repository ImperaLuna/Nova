const TAGS: [&str; 6] = ["b", "i", "u", "a", "img", "br"];
const ATTRIBUTED: [&str; 2] = ["a", "img"];
const MAX_TAG_BYTES: usize = 2048;
const MARKUP_BYTES_PER_CHAR: usize = 64;

pub fn plain(markup: &str, max_chars: usize) -> String {
    let markup = prefix(markup, MARKUP_BYTES_PER_CHAR * max_chars);
    let mut text = String::with_capacity(markup.len());
    let mut lead = 0;
    let mut rest = markup;
    while let Some(at) = rest.find(['<', '&']) {
        lead = text.len() - text[lead..].trim_start().len();
        if text.len() - lead > 4 * max_chars {
            rest = "";
            break;
        }
        text.push_str(&rest[..at]);
        rest = &rest[at..];
        let (replacement, length) = if rest.starts_with('<') { tag(rest) } else { entity(rest) };
        match replacement {
            Some(replacement) => {
                text.push_str(replacement.as_deref().unwrap_or_default());
                rest = &rest[length..];
            }
            None => {
                text.push_str(&rest[..1]);
                rest = &rest[1..];
            }
        }
    }
    text.push_str(rest);
    cut(text.trim(), max_chars)
}

pub fn cut(text: &str, max_chars: usize) -> String {
    let mut kept = text.chars().filter(|&c| !c.is_control() || matches!(c, '\n' | '\t'));
    let mut cut: String = kept.by_ref().take(max_chars).collect();
    if kept.next().is_some() {
        cut.pop();
        cut.push('…');
    }
    cut
}

fn prefix(text: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn tag(text: &str) -> (Option<Option<String>>, usize) {
    let window = prefix(text, MAX_TAG_BYTES);
    let Some(end) = window.find('>') else { return (None, 0) };
    let inside = text[1..end].trim_start_matches('/').trim_end_matches('/').trim_end();
    let (name, attributes) = inside.split_once(|c: char| c.is_ascii_whitespace()).unwrap_or((inside, ""));
    if !TAGS.iter().any(|tag| tag.eq_ignore_ascii_case(name)) {
        return (None, 0);
    }
    if !attributes.is_empty() && !ATTRIBUTED.iter().any(|tag| tag.eq_ignore_ascii_case(name)) {
        return (None, 0);
    }
    let replacement = name.eq_ignore_ascii_case("br").then(|| "\n".to_string());
    (Some(replacement), end + 1)
}

fn entity(text: &str) -> (Option<Option<String>>, usize) {
    let Some(end) = text.char_indices().take(10).find(|(_, c)| *c == ';').map(|(at, _)| at) else {
        return (None, 0);
    };
    let name = &text[1..end];
    let character = match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => name.strip_prefix('#').and_then(|number| {
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => number.parse().ok(),
            };
            code.and_then(char::from_u32)
        }),
    };
    match character {
        Some(character) => (Some(Some(character.to_string())), end + 1),
        None => (None, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tags_go_and_entities_decode() {
        let body = r#"<b>Mara</b>: see <a href="https://example.com">the map</a><br/>it's &lt;3 &amp; &#x1F5FA;"#;
        assert_eq!(plain(body, 200), "Mara: see the map\nit's <3 & 🗺");
    }

    #[test]
    fn a_long_run_of_unclosed_tags_takes_linear_time() {
        let body = "<".repeat(512 * 1024);
        let started = std::time::Instant::now();
        let text = plain(&body, 2048);
        assert_eq!(text.chars().count(), 2048);
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
    }

    #[test]
    fn leading_whitespace_between_tags_takes_linear_time() {
        let body = format!("{}{}", " ".repeat(80_000), "<b>".repeat(80_000));
        let started = std::time::Instant::now();
        assert_eq!(plain(&body, 2048), "");
        assert!(started.elapsed() < std::time::Duration::from_secs(1), "{:?}", started.elapsed());
    }

    #[test]
    fn a_huge_body_is_read_only_as_far_as_it_can_show() {
        let body = format!("{}visible", "<b>".repeat(4 * 1024 * 1024));
        let started = std::time::Instant::now();
        assert!(!plain(&body, 2048).contains("visible"));
        assert!(started.elapsed() < std::time::Duration::from_secs(1), "{:?}", started.elapsed());
        assert_eq!(plain(&format!("{}visible", "<b>".repeat(16)), 2048), "visible");
    }

    #[test]
    fn control_characters_go() {
        assert_eq!(plain("a\u{1}b\u{7f}c\nd\te", 200), "abc\nd\te");
        assert_eq!(cut("\u{1b}[31mred", 200), "[31mred");
        assert_eq!(cut("ab\u{1}\u{1}", 2), "ab");
    }

    #[test]
    fn plain_text_that_looks_like_markup_stays() {
        assert_eq!(plain("if a <b or c > d && x &unknown; <div>", 200), "if a <b or c > d && x &unknown; <div>");
        assert_eq!(plain("a < b", 200), "a < b");
    }

    #[test]
    fn long_text_is_cut_with_an_ellipsis() {
        assert_eq!(plain("abcdef", 4), "abc…");
        assert_eq!(plain("  abc  ", 4), "abc");
        assert_eq!(cut("äöüß", 3), "äö…");
    }
}
