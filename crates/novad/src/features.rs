//! Which features the UI may load: on in the user's selection and with all
//! their binaries present.

use nova_features::{Manifest, Selection};
use nova_protocol::{FeatureState, FeatureStatus};
use std::path::Path;

/// The user's choices in `path` (`features.toml`); a missing file means
/// each feature's default.
pub fn read_selection(path: &Path) -> Result<Selection, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Selection::default()),
        Err(e) => return Err(format!("can't read {}: {e}", path.display())),
    };
    Selection::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn states(manifests: &[Manifest], selection: &Selection, path_var: &str) -> Vec<FeatureState> {
    manifests
        .iter()
        .map(|m| {
            if !selection.is_on(m) {
                return FeatureState { id: m.id.clone(), status: FeatureStatus::Off, missing: vec![] };
            }
            let missing = m.missing_binaries(path_var);
            let status = if missing.is_empty() { FeatureStatus::Ready } else { FeatureStatus::MissingDeps };
            FeatureState { id: m.id.clone(), status, missing }
        })
        .collect()
}

/// Whether feature `id` is on with all it needs, so the UI loads it and
/// novad runs its service.
pub fn is_ready(features: &[FeatureState], id: &str) -> bool {
    features.iter().any(|feature| feature.id == id && feature.status == FeatureStatus::Ready)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_follow_selection_and_path() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        let manifests = nova_features::load_all(&root).unwrap();
        let on = Selection::parse("enabled = [\"recording\"]").unwrap();
        let states = states(&manifests, &on, "/nonexistent");
        let rec = states.iter().find(|s| s.id == "recording").unwrap();
        assert_eq!(rec.status, FeatureStatus::MissingDeps);
        let off = super::states(&manifests, &Selection::parse("disabled = [\"recording\"]").unwrap(), "/nonexistent");
        assert_eq!(off.iter().find(|s| s.id == "recording").unwrap().status, FeatureStatus::Off);
    }
}
