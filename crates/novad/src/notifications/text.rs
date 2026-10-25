//! Notification text as the UI shows it: plain, and not too long. Apps may
//! send a little markup in the body (`<b>`, `<i>`, `<u>`, `<a href>`,
//! `<img>`, `<br>`) with entities like `&amp;`; the tags go and the entities
//! become their characters. Anything else that looks like a tag stays as
//! text, since an app that sends plain text may well mean `a <b` literally.

/// The tags the notification spec allows in a body. `br` isn't one, but
/// apps send it for a line break.
const TAGS: [&str; 6] = ["b", "i", "u", "a", "img", "br"];
/// The tags among them that take attributes (`href`, `src`, `alt`).
const ATTRIBUTED: [&str; 2] = ["a", "img"];
/// A tag's `>` is looked for this far (in bytes) at most, so a body of
/// `<`s without one takes linear time.
const MAX_TAG_BYTES: usize = 2048;

/// `markup` without its tags and with its entities decoded, cut to
/// `max_chars` characters (an ellipsis marks the cut).
pub fn plain(markup: &str, max_chars: usize) -> String {
    let mut text = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(at) = rest.find(['<', '&']) {
        // Past what's kept, at four bytes a character at most: the rest is cut anyway.
        if text.trim_start().len() > 4 * max_chars {
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
                // Not markup: the character stays as text.
                text.push_str(&rest[..1]);
                rest = &rest[1..];
            }
        }
    }
    text.push_str(rest);
    cut(text.trim(), max_chars)
}

/// `text` cut to `max_chars` characters, with an ellipsis as the last one
/// when anything was cut.
pub fn cut(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// A known tag at the start of `text` (which starts with `<`): what it
/// becomes (a line break for `<br>`, nothing for the others) and how long
/// it is. `None` when it isn't one.
fn tag(text: &str) -> (Option<Option<String>>, usize) {
    let mut window = MAX_TAG_BYTES.min(text.len());
    while !text.is_char_boundary(window) {
        window -= 1;
    }
    let window = &text[..window];
    let Some(end) = window.find('>') else { return (None, 0) };
    let inside = text[1..end].trim_start_matches('/').trim_end_matches('/').trim_end();
    let (name, attributes) = inside.split_once(|c: char| c.is_ascii_whitespace()).unwrap_or((inside, ""));
    if !TAGS.iter().any(|tag| tag.eq_ignore_ascii_case(name)) {
        return (None, 0);
    }
    // Only links and pictures take attributes: `a <b or c > d` is text.
    if !attributes.is_empty() && !ATTRIBUTED.iter().any(|tag| tag.eq_ignore_ascii_case(name)) {
        return (None, 0);
    }
    let replacement = name.eq_ignore_ascii_case("br").then(|| "\n".to_string());
    (Some(replacement), end + 1)
}

/// An entity at the start of `text` (which starts with `&`): its character
/// and its length. `None` when it isn't one.
fn entity(text: &str) -> (Option<Option<String>>, usize) {
    // The longest named or numeric entity taken here is `&#x10FFFF;`.
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
