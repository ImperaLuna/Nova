use std::path::PathBuf;

const MAX_PATH_BYTES: usize = 4096;
const MAX_REFERENCE_BYTES: usize = "file://".len() + 3 * MAX_PATH_BYTES;

pub fn file(reference: &str) -> Option<PathBuf> {
    if reference.len() > MAX_REFERENCE_BYTES {
        return None;
    }
    let path = match reference.strip_prefix("file://") {
        Some(uri) => PathBuf::from(decode_percents(uri)?),
        None => PathBuf::from(reference),
    };
    (path.is_absolute() && path.as_os_str().len() <= MAX_PATH_BYTES).then_some(path)
}

fn decode_percents(uri: &str) -> Option<String> {
    String::from_utf8(percent_decoded(uri)?).ok()
}

pub fn percent_decoded(uri: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(uri.len());
    let mut rest = uri.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(after.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &after[2..];
        } else {
            bytes.push(byte);
            rest = after;
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_paths_or_file_uris() {
        assert_eq!(file("file:///tmp/a.png"), Some(PathBuf::from("/tmp/a.png")));
        assert_eq!(file("/tmp/a.png"), Some(PathBuf::from("/tmp/a.png")));
        assert_eq!(file("discord"), None);
        assert_eq!(file("relative/a.png"), None);
        assert_eq!(file("file:///tmp/avatar%20photo.png"), Some(PathBuf::from("/tmp/avatar photo.png")));
        assert_eq!(file("/tmp/100%25.png"), Some(PathBuf::from("/tmp/100%25.png")));
        assert_eq!(file("file:///tmp/%zz.png"), None);
    }

    #[test]
    fn paths_too_long_to_be_paths_are_refused() {
        let longest = format!("/{}", "a".repeat(MAX_PATH_BYTES - 1));
        assert_eq!(file(&longest), Some(PathBuf::from(&longest)));
        assert_eq!(file(&format!("{longest}a")), None);
        assert_eq!(file(&format!("/{}", "a".repeat(4 * 1024 * 1024))), None);
        let escaped = format!("file:///{}", "%20".repeat(MAX_PATH_BYTES - 1));
        assert_eq!(file(&escaped), Some(PathBuf::from(format!("/{}", " ".repeat(MAX_PATH_BYTES - 1)))));
        assert_eq!(file(&format!("{escaped}%20")), None);
    }
}
