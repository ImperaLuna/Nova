use nova_base::{lock, state_file};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Component, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const MAX_COVER_BYTES: u64 = 2 * 1024 * 1024;
const MAX_KEPT: usize = 8;
const TIMEOUT: Duration = Duration::from_secs(10);
const USER_AGENT: &str = concat!("nova/", env!("CARGO_PKG_VERSION"));
const FOLDER_MODE: u32 = 0o700;
const FILE_SCHEME: &str = "file://";
const HTTPS_SCHEME: &str = "https://";

pub struct Covers {
    folder: PathBuf,
    kept: Mutex<VecDeque<(String, Option<PathBuf>)>>,
}

impl Covers {
    pub fn new(folder: PathBuf) -> Self {
        if let Ok(entries) = std::fs::read_dir(&folder) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Self { folder, kept: Mutex::new(VecDeque::new()) }
    }

    pub fn downloads(url: &str) -> bool {
        url.starts_with(HTTPS_SCHEME)
    }

    pub fn file(&self, url: &str) -> Result<Option<PathBuf>, String> {
        if url.starts_with(FILE_SCHEME) {
            return local(url).map(Some);
        }
        if !url.starts_with(HTTPS_SCHEME) {
            return Ok(None);
        }
        {
            let mut kept = lock(&self.kept);
            if let Some(entry) = kept.iter().position(|(kept, _)| kept == url).and_then(|at| kept.remove(at)) {
                let file = entry.1.clone();
                kept.push_back(entry);
                return Ok(file);
            }
        }
        let downloaded = self.download(url);
        self.keep(url, downloaded.as_ref().ok().cloned());
        downloaded.map(Some)
    }

    fn download(&self, url: &str) -> Result<PathBuf, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .https_only(true)
            .user_agent(USER_AGENT)
            .build()
            .into();
        let mut response = agent.get(url).call().map_err(|e| format!("can't download {url}: {e}"))?;
        let kind = response.headers().get("content-type").and_then(|value| value.to_str().ok()).unwrap_or_default();
        let extension = extension(kind).ok_or_else(|| format!("{url} isn't an image ({kind})"))?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_COVER_BYTES)
            .read_to_vec()
            .map_err(|e| format!("can't download {url}: {e}"))?;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(FOLDER_MODE)
            .create(&self.folder)
            .map_err(|e| format!("can't make {}: {e}", self.folder.display()))?;
        let path = self.folder.join(format!("{:016x}.{extension}", hash(url)));
        state_file::write(&path, &bytes).map_err(|e| format!("can't write {}: {e}", path.display()))?;
        Ok(path)
    }

    fn keep(&self, url: &str, file: Option<PathBuf>) {
        let mut kept = lock(&self.kept);
        kept.retain(|(other, _)| other != url);
        kept.push_back((url.to_string(), file));
        while kept.len() > MAX_KEPT {
            let Some((_, Some(old))) = kept.pop_front() else { continue };
            if !kept.iter().any(|(_, file)| file.as_ref() == Some(&old)) {
                let _ = std::fs::remove_file(old);
            }
        }
    }
}

fn local(url: &str) -> Result<PathBuf, String> {
    let path = nova_paths::reference::file(url).ok_or_else(|| format!("{url} isn't a file path"))?;
    if path.components().any(|component| component == Component::ParentDir) {
        return Err(format!("{} has .. in it", path.display()));
    }
    let metadata = std::fs::metadata(&path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} isn't a file", path.display()));
    }
    if metadata.len() > MAX_COVER_BYTES {
        return Err(format!("{} is over {MAX_COVER_BYTES} bytes", path.display()));
    }
    Ok(path)
}

fn extension(content_type: &str) -> Option<&'static str> {
    match content_type.split(';').next()?.trim() {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

fn hash(url: &str) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    url.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_is_downloaded() {
        let dir = nova_test_support::TempDir::new("covers-schemes");
        let covers = Covers::new(dir.join("media"));
        assert_eq!(covers.file("http://example.com/cover.jpg"), Ok(None));
        assert_eq!(covers.file("data:image/png;base64,AAAA"), Ok(None));
        assert_eq!(covers.file("ftp://example.com/cover.jpg"), Ok(None));
        assert!(!dir.join("media").exists(), "nothing downloads for those");
    }

    #[test]
    fn local_covers_must_be_small_regular_files() {
        let dir = nova_test_support::TempDir::new("covers-local");
        let covers = Covers::new(dir.join("media"));
        let cover = dir.join("cover.png");
        std::fs::write(&cover, b"png").unwrap();
        assert_eq!(covers.file(&format!("file://{}", cover.display())), Ok(Some(cover.clone())));
        assert!(covers.file(&format!("file://{}/../cover.png", dir.join("media").display())).is_err());
        assert!(covers.file(&format!("file://{}", dir.display())).is_err(), "a folder");
        assert!(covers.file("file://relative/cover.png").is_err());
        assert!(covers.file(&format!("file://{}", dir.join("missing.png").display())).is_err());
        let big = dir.join("big.png");
        std::fs::File::create(&big).unwrap().set_len(MAX_COVER_BYTES + 1).unwrap();
        assert!(covers.file(&format!("file://{}", big.display())).is_err());
    }

    #[test]
    fn only_image_types_are_kept() {
        assert_eq!(extension("image/jpeg"), Some("jpg"));
        assert_eq!(extension("image/png; charset=binary"), Some("png"));
        assert_eq!(extension("text/html"), None);
        assert_eq!(extension("image/svg+xml"), None);
    }

    #[test]
    fn old_downloads_are_deleted() {
        let dir = nova_test_support::TempDir::new("covers-kept");
        let folder = dir.join("media");
        std::fs::create_dir_all(&folder).unwrap();
        let covers = Covers::new(folder.clone());
        let files: Vec<PathBuf> = (0..MAX_KEPT + 2).map(|n| folder.join(format!("{n}.jpg"))).collect();
        for (n, file) in files.iter().enumerate() {
            std::fs::write(file, b"jpg").unwrap();
            covers.keep(&format!("https://example.com/{n}.jpg"), Some(file.clone()));
        }
        assert!(!files[0].exists() && !files[1].exists());
        assert!(files[2..].iter().all(|file| file.exists()));
        assert_eq!(covers.file("https://example.com/2.jpg"), Ok(Some(files[2].clone())));
        let newer = folder.join("newer.jpg");
        std::fs::write(&newer, b"jpg").unwrap();
        covers.keep("https://example.com/newer.jpg", Some(newer));
        assert!(files[2].exists() && !files[3].exists());
    }

    #[test]
    fn earlier_covers_are_cleared_at_start() {
        let dir = nova_test_support::TempDir::new("covers-start");
        let folder = dir.join("media");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("old.jpg"), b"jpg").unwrap();
        let _covers = Covers::new(folder.clone());
        assert!(!folder.join("old.jpg").exists());
    }
}
