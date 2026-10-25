//! What features keep across restarts, in a JSON file of their own in the
//! state folder: private to the user, and replaced in one step, so a crash
//! mid-save never leaves half a file.

use serde::Serialize;
use std::path::Path;

/// The saved state is the user's own business.
const MODE: u32 = 0o600;

/// Writes a private temp file next to `path` and renames it over `path`.
pub fn save(path: &Path, saved: &impl Serialize) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("json.partial");
    // One left by a crash, or anything else in its place, goes first:
    // `create_new` never follows a link or reuses a file.
    match std::fs::remove_file(&partial) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(MODE).open(&partial)?;
    file.write_all(&serde_json::to_vec_pretty(saved).map_err(std::io::Error::other)?)?;
    std::fs::rename(partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn saves_privately_over_a_stale_partial_file() {
        let dir = std::env::temp_dir().join(format!("novad-state-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("feature.json");
        save(&path, &serde_json::json!({"a": 1})).unwrap();
        std::fs::write(path.with_extension("json.partial"), "left by a crash").unwrap();
        save(&path, &serde_json::json!({"a": 2})).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["a"], 2);
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, MODE);
        assert!(!path.with_extension("json.partial").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
