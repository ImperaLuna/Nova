use serde::Serialize;
use std::path::Path;

const MODE: u32 = 0o600;

pub fn save(path: &Path, saved: &impl Serialize) -> std::io::Result<()> {
    write(path, &serde_json::to_vec_pretty(saved).map_err(std::io::Error::other)?)
}

pub fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    let partial = std::path::PathBuf::from(partial);
    // `create_new` fails on any entry already there, a link included:
    // https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new
    match std::fs::remove_file(&partial) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(MODE).open(&partial)?;
    file.write_all(bytes)?;
    std::fs::rename(partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn saves_privately_over_a_stale_partial_file() {
        let dir = nova_test_support::TempDir::new("state-file");
        let path = dir.join("feature.json");
        save(&path, &serde_json::json!({"a": 1})).unwrap();
        std::fs::write(path.with_extension("json.partial"), "left by a crash").unwrap();
        save(&path, &serde_json::json!({"a": 2})).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["a"], 2);
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, MODE);
        assert!(!path.with_extension("json.partial").exists());
    }
}
