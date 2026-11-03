//! Valve's KeyValues text format (`.vdf`, `.acf`), as Steam writes
//! `libraryfolders.vdf` and its `appmanifest_*.acf` files: quoted (or bare)
//! keys, each followed by a quoted text or a `{ }` block of more of them,
//! with `//` comments. Keys are matched case aside, as Steam does.
//!
//! Only what Steam writes is read: a file is read up to [`MAX_BYTES`] and
//! blocks nest at most [`MAX_DEPTH`] deep; anything else is an error, so a
//! broken file is skipped rather than half read.

use std::io::Read;
use std::path::Path;

/// More than any library list or app manifest holds.
const MAX_BYTES: u64 = 1 << 20;
/// Deeper than Steam's files nest.
const MAX_DEPTH: usize = 16;

/// A key's value: text, or a block of more keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Block(Block),
}

/// Keys and values in the file's order.
pub type Block = Vec<(String, Value)>;

/// The value under `key` in `block`, case aside.
pub fn get<'a>(block: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    block.iter().find(|(name, _)| name.eq_ignore_ascii_case(key)).map(|(_, value)| value)
}

/// The text under `key` in `block`.
pub fn text<'a>(block: &'a [(String, Value)], key: &str) -> Option<&'a str> {
    match get(block, key)? {
        Value::Text(text) => Some(text),
        Value::Block(_) => None,
    }
}

/// The block under `key` in `block`.
pub fn block<'a>(block: &'a [(String, Value)], key: &str) -> Option<&'a Block> {
    match get(block, key)? {
        Value::Block(inner) => Some(inner),
        Value::Text(_) => None,
    }
}

/// The file at `path`, read and parsed.
pub fn read(path: &Path) -> Result<Block, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.take(MAX_BYTES + 1).read_to_string(&mut text).map_err(|e| e.to_string())?;
    if text.len() as u64 > MAX_BYTES {
        return Err(format!("longer than {MAX_BYTES} bytes"));
    }
    parse(&text)
}

/// `text` as its top-level keys.
pub fn parse(text: &str) -> Result<Block, String> {
    Tokens { rest: text }.block(0)
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    Text(String),
}

struct Tokens<'a> {
    rest: &'a str,
}

impl Tokens<'_> {
    /// Keys and values up to the `}` that ends the block, which it reads,
    /// or, at `depth` 0, up to the end.
    fn block(&mut self, depth: usize) -> Result<Block, String> {
        if depth > MAX_DEPTH {
            return Err(format!("blocks nested deeper than {MAX_DEPTH}"));
        }
        let nested = depth > 0;
        let mut block = Vec::new();
        loop {
            let key = match self.next()? {
                None if nested => return Err("a block with no }".into()),
                Some(Token::Close) if !nested => return Err("a } with no { before it".into()),
                None | Some(Token::Close) => return Ok(block),
                Some(Token::Open) => return Err("a { where a key should be".into()),
                Some(Token::Text(key)) => key,
            };
            let value = match self.next()? {
                Some(Token::Text(text)) => Value::Text(text),
                Some(Token::Open) => Value::Block(self.block(depth + 1)?),
                Some(Token::Close) | None => return Err(format!("{key} has no value")),
            };
            block.push((key, value));
        }
    }

    fn next(&mut self) -> Result<Option<Token>, String> {
        loop {
            self.rest = self.rest.trim_start();
            match self.rest.strip_prefix("//") {
                Some(comment) => self.rest = comment.split_once('\n').map_or("", |(_, after)| after),
                None => break,
            }
        }
        let mut chars = self.rest.chars();
        let token = match chars.next() {
            None => return Ok(None),
            Some('{') => Token::Open,
            Some('}') => Token::Close,
            Some('"') => {
                let mut text = String::new();
                loop {
                    match chars.next() {
                        None => return Err("a quote that doesn't end".into()),
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => text.push('\n'),
                            Some('t') => text.push('\t'),
                            Some(other @ ('\\' | '"')) => text.push(other),
                            Some(other) => {
                                text.push('\\');
                                text.push(other);
                            }
                            None => return Err("a quote that doesn't end".into()),
                        },
                        Some(other) => text.push(other),
                    }
                }
                Token::Text(text)
            }
            Some(_) => {
                let end = self.rest.find(|c: char| c.is_whitespace() || "{}\"".contains(c)).unwrap_or(self.rest.len());
                let bare = &self.rest[..end];
                self.rest = &self.rest[end..];
                return Ok(Some(Token::Text(bare.to_owned())));
            }
        };
        self.rest = chars.as_str();
        Ok(Some(token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_hold_text_or_blocks() {
        let parsed = parse(
            "\"AppState\"\n{\n\t\"appid\"\t\t\"1245620\"\n\t\"name\"\t\t\"ELDEN RING\"\n\t\"UserConfig\"\n\t{\n\t\t\"language\"\t\t\"english\"\n\t}\n}\n",
        )
        .unwrap();
        let app = block(&parsed, "appstate").unwrap();
        assert_eq!(text(app, "AppID"), Some("1245620"));
        assert_eq!(text(app, "name"), Some("ELDEN RING"));
        assert_eq!(text(block(app, "userconfig").unwrap(), "language"), Some("english"));
        assert_eq!(text(app, "UserConfig"), None, "a block isn't text");
    }

    #[test]
    fn escapes_comments_and_bare_words_read_like_steam() {
        let parsed = parse("// written by Steam\nlibraryfolders { \"0\" { path \"D:\\\\Games \\\"x\\\"\" } }").unwrap();
        let library = block(block(&parsed, "libraryfolders").unwrap(), "0").unwrap();
        assert_eq!(text(library, "path"), Some("D:\\Games \"x\""));
    }

    #[test]
    fn a_broken_file_is_an_error() {
        for broken in ["\"a\" {", "\"a\"", "\"a\" \"b", "}", "{ \"a\" \"b\" }"] {
            assert!(parse(broken).is_err(), "{broken}");
        }
        let deep = "\"a\" {".repeat(MAX_DEPTH + 2) + &"}".repeat(MAX_DEPTH + 2);
        assert!(parse(&deep).unwrap_err().contains("deeper"));
    }
}
