//! Covers (`mpris:artUrl`): a `file://` cover is passed through after the
//! checks notification images get (a regular file, not too big); an
//! `https://` one is downloaded once into novad's runtime folder, with the
//! same HTTPS-only, timeout and size-cap rules as the other network calls.
//! Other schemes (and `http://`, data URLs) have no cover.

use super::{COVER_KEEP, COVER_MAX_BYTES};
use std::collections::hash_map::DefaultHasher;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How long a cover download may take.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10);
/// The image types a cover may be, by extension; anything else is refused.
const IMAGE_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
/// Longest cover URL kept; longer ones have no cover.
const MAX_URL_BYTES: usize = 2048;
const PRIVATE_FILE_MODE: u32 = 0o600;
const PRIVATE_DIR_MODE: u32 = 0o700;

/// Where a cover comes from, as `mpris:artUrl` names it.
#[derive(Debug, PartialEq)]
pub enum Source {
    /// A file already on disk, which the UI can show as it is.
    Local(PathBuf),
    /// An HTTPS image, fetched into the cache folder first.
    Remote(String),
}

/// The source of `url`, or `None` when it has no usable cover.
pub fn source(url: &str) -> Option<Source> {
    if url.len() > MAX_URL_BYTES {
        return None;
    }
    if url.starts_with("https://") {
        return Some(Source::Remote(url.to_string()));
    }
    let path = crate::notifications::image::file(url)?;
    let regular = path.metadata().is_ok_and(|meta| meta.is_file() && meta.len() <= COVER_MAX_BYTES);
    regular.then_some(Source::Local(path))
}

/// The cached file a remote cover is saved as: a hash of its URL and its
/// image extension, so a track's cover is downloaded once.
pub fn cached_name(url: &str) -> Option<String> {
    let extension = url.rsplit('/').next()?.rsplit('.').next()?.to_lowercase();
    if !IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    Some(format!("{:016x}.{extension}", hasher.finish()))
}

/// Downloads `url` into `folder` (created private) and returns the file,
/// or the reason it failed. Older covers beyond [`COVER_KEEP`] are removed.
pub fn fetch(url: &str, folder: &Path) -> Result<PathBuf, String> {
    let name = cached_name(url).ok_or("the cover isn't a JPEG, PNG or WebP image")?;
    let path = folder.join(&name);
    if path.is_file() {
        return Ok(path);
    }
    std::fs::DirBuilder::new().recursive(true).mode(PRIVATE_DIR_MODE).create(folder).map_err(|e| e.to_string())?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .http_status_as_error(true)
        .max_redirects(0)
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    let bytes = response.body_mut().with_config().limit(COVER_MAX_BYTES).read_to_vec().map_err(|e| e.to_string())?;
    // Written beside the target and renamed over it, so a half-written cover is never read.
    let temp = folder.join(format!(".{name}.part"));
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(PRIVATE_FILE_MODE)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes).and_then(|()| file.sync_all()).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, &path).map_err(|e| e.to_string())?;
    prune(folder, &path);
    Ok(path)
}

/// Removes the oldest covers so that at most [`COVER_KEEP`] remain, never `keep`.
fn prune(folder: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    let mut covers: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.as_path() != keep)
        .filter_map(|path| Some((path.metadata().ok()?.modified().ok()?, path)))
        .collect();
    covers.sort();
    let excess = (covers.len() + 1).saturating_sub(COVER_KEEP);
    for (_, path) in covers.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_is_fetched_and_only_images_are_cached() {
        assert_eq!(source("https://example.com/a.png"), Some(Source::Remote("https://example.com/a.png".into())));
        assert_eq!(source("http://example.com/a.png"), None);
        assert_eq!(source("data:image/png;base64,AAAA"), None);
        assert!(cached_name("https://example.com/a.PNG").is_some_and(|name| name.ends_with(".png")));
        assert_eq!(cached_name("https://example.com/a.exe"), None);
        assert_eq!(cached_name("https://example.com/a"), None);
    }

    #[test]
    fn oversized_urls_have_no_cover() {
        let url = format!("https://example.com/{}.png", "a".repeat(MAX_URL_BYTES));
        assert_eq!(source(&url), None);
    }
}
