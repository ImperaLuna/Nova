//! Which features the UI may load: on in the user's selection and with all
//! their binaries present.

use nova_features::{Manifest, Selection};
use nova_protocol::{FeatureState, FeatureStatus};
use std::path::PathBuf;

/// The user's choices in the first of `paths` that exists (`features.toml`,
/// see [`nova_paths::config_files`]); none means each feature's default.
pub fn read_selection(paths: &[PathBuf]) -> Result<Selection, String> {
    for path in paths {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("can't read {}: {e}", path.display())),
        };
        return Selection::parse(&text).map_err(|e| format!("{}: {e}", path.display()));
    }
    Ok(Selection::default())
}

/// Each feature's state (off, missing what it needs, or ready) and its
/// slots, for the UI to fill its hosts with.
pub fn states(manifests: &[Manifest], selection: &Selection, path_var: &str) -> Vec<FeatureState> {
    manifests
        .iter()
        .map(|m| {
            let (status, missing) = if !selection.is_on(m) {
                (FeatureStatus::Off, vec![])
            } else {
                let missing = m.missing_binaries(path_var);
                let status = if missing.is_empty() { FeatureStatus::Ready } else { FeatureStatus::MissingDeps };
                (status, missing)
            };
            FeatureState { id: m.id.clone(), status, missing, slots: m.slots.clone() }
        })
        .collect()
}

/// Whether feature `id` is on with all it needs, so the UI loads it and
/// novad runs its service.
pub fn is_ready(features: &[FeatureState], id: &str) -> bool {
    features.iter().any(|feature| feature.id == id && feature.status == FeatureStatus::Ready)
}

/// The error for a request that needs feature `id` while it's not ready
/// (off, missing what it needs, or stopped by a reload).
pub fn not_ready(id: &str) -> String {
    format!("the {id} feature isn't ready (see `nova status`)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_selection_file_there_wins() {
        let dir = nova_test_support::TempDir::new("selection-order");
        let (user, system) = (dir.join("user.toml"), dir.join("system.toml"));
        let paths = [user.clone(), system.clone()];
        assert_eq!(read_selection(&paths), Ok(Selection::default()));
        std::fs::write(&system, "enabled = [\"recording\"]\n").unwrap();
        assert_eq!(read_selection(&paths).unwrap().enabled, ["recording"]);
        std::fs::write(&user, "disabled = [\"recording\"]\n").unwrap();
        let selection = read_selection(&paths).unwrap();
        assert!(selection.enabled.is_empty());
        assert_eq!(selection.disabled, ["recording"]);
    }

    #[test]
    fn states_follow_selection_and_path() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        let manifests = nova_features::load_all(&root).unwrap();
        let on = Selection::parse("enabled = [\"recording\"]").unwrap();
        let states = states(&manifests, &on, "/nonexistent");
        let rec = states.iter().find(|s| s.id == "recording").unwrap();
        assert_eq!(rec.status, FeatureStatus::MissingDeps);
        let off = super::states(&manifests, &Selection::parse("disabled = [\"recording\"]").unwrap(), "/nonexistent");
        assert_eq!(off.iter().find(|s| s.id == "recording").unwrap().status, FeatureStatus::Off);
    }

    #[test]
    fn states_carry_the_slots() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        let manifests = nova_features::load_all(&root).unwrap();
        let states = states(&manifests, &Selection::default(), "/nonexistent");
        let core = states.iter().find(|s| s.id == "core").unwrap();
        assert_eq!(core.slots, manifests.iter().find(|m| m.id == "core").unwrap().slots);
    }
}
