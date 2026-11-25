use nova_daemon_info::{FeatureState, FeatureStatus};
use nova_features::{Manifest, Selection};
use std::path::PathBuf;

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

pub fn states(manifests: &[Manifest], selection: &Selection, path_var: &str) -> Vec<FeatureState> {
    manifests
        .iter()
        .map(|m| {
            let (status, missing, mode) = if !selection.is_on(m) {
                (FeatureStatus::Off, vec![], None)
            } else {
                let missing = m.missing_binaries(path_var);
                let status = if missing.is_empty() { FeatureStatus::Ready } else { FeatureStatus::MissingDeps };
                (status, missing, selection.mode(m).map(str::to_string))
            };
            FeatureState { id: m.id.clone(), status, missing, slots: m.slots.clone(), mode }
        })
        .collect()
}

pub fn is_ready(features: &[FeatureState], id: &str) -> bool {
    features.iter().any(|feature| feature.id == id && feature.status == FeatureStatus::Ready)
}

pub fn needs_ui(manifests: &[Manifest], features: &[FeatureState]) -> bool {
    manifests.iter().any(|manifest| manifest.has_ui() && is_ready(features, &manifest.id))
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
    fn the_ui_runs_only_while_a_feature_that_draws_is_ready() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../features");
        let manifests = nova_features::load_all(&root).unwrap();
        let ids = |selection: &str| states(&manifests, &Selection::parse(selection).unwrap(), "/nonexistent");
        assert!(needs_ui(&manifests, &ids("")), "the defaults draw the bar");
        let every_id: Vec<String> = manifests.iter().map(|m| format!("{:?}", m.id)).collect();
        let all_off = format!("disabled = [{}]", every_id.join(", "));
        assert!(!needs_ui(&manifests, &ids(&all_off)));
        let only_bar = all_off.replace("\"bar\", ", "");
        assert!(needs_ui(&manifests, &ids(&only_bar)), "{only_bar}");
    }

    #[test]
    fn states_follow_selection_and_path() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../features");
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
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../features");
        let manifests = nova_features::load_all(&root).unwrap();
        let states = states(&manifests, &Selection::default(), "/nonexistent");
        let bar = states.iter().find(|s| s.id == "bar").unwrap();
        assert_eq!(bar.slots, manifests.iter().find(|m| m.id == "bar").unwrap().slots);
    }

    #[test]
    fn states_carry_the_mode_while_on() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../features");
        let manifests = nova_features::load_all(&root).unwrap();
        let mode = |selection: &str| {
            let states = states(&manifests, &Selection::parse(selection).unwrap(), "/nonexistent");
            states.into_iter().find(|s| s.id == "nova-files").unwrap().mode
        };
        assert_eq!(mode(""), None, "off by default");
        assert_eq!(mode("enabled = [\"nova-files\"]").as_deref(), Some("basic"));
        for picked in ["launcher", "basic", "full"] {
            let selection = format!("enabled = [\"nova-files\"]\n[modes]\nnova-files = \"{picked}\"");
            assert_eq!(mode(&selection).as_deref(), Some(picked));
        }
        assert_eq!(mode("enabled = [\"recording\"]\n[modes]\nnova-files = \"full\""), None);
    }
}
