//! Which features the UI may load: on in the user's selection and with all
//! their binaries present.

use nova_features::{Manifest, Selection};
use nova_protocol::{FeatureState, FeatureStatus};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn states_follow_selection_and_path() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        let manifests = nova_features::load_all(&root).unwrap();
        let on = Selection::parse("enabled = [\"recording\"]").unwrap();
        let states = states(&manifests, &on, "/nonexistent");
        let rec = states.iter().find(|s| s.id == "recording").unwrap();
        assert_eq!(rec.status, FeatureStatus::MissingDeps);
        let off = super::states(&manifests, &Selection::default(), "/nonexistent");
        assert_eq!(off.iter().find(|s| s.id == "recording").unwrap().status, FeatureStatus::Off);
    }
}
