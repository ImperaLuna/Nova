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

/// Each feature's state: off, missing what it needs, with no host for its
/// slots (every one names a host that doesn't exist or whose feature isn't
/// ready), or ready.
pub fn states(manifests: &[Manifest], selection: &Selection, path_var: &str) -> Vec<FeatureState> {
    let mut states: Vec<FeatureState> = manifests
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
        .collect();
    // Judged from the states above, so a host's owner that has no host
    // itself doesn't chain on to the features it hosts.
    let hosted: Vec<bool> = states
        .iter()
        .map(|state| {
            state.slots.is_empty()
                || state.slots.iter().any(|slot| {
                    nova_features::host(&slot.host)
                        .is_some_and(|host| host.owner.is_none_or(|owner| is_ready(&states, owner)))
                })
        })
        .collect();
    for (state, hosted) in states.iter_mut().zip(hosted) {
        if state.status == FeatureStatus::Ready && !hosted {
            state.status = FeatureStatus::NoHost;
        }
    }
    states
}

/// Whether feature `id` is on with all it needs, so the UI loads it and
/// novad runs its service.
pub fn is_ready(features: &[FeatureState], id: &str) -> bool {
    features.iter().any(|feature| feature.id == id && feature.status == FeatureStatus::Ready)
}

/// Whether the UI has to run: some feature that draws is ready.
pub fn needs_ui(manifests: &[Manifest], features: &[FeatureState]) -> bool {
    manifests.iter().any(|manifest| manifest.has_ui() && is_ready(features, &manifest.id))
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
    fn the_ui_runs_only_while_a_feature_that_draws_is_ready() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
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
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        let manifests = nova_features::load_all(&root).unwrap();
        let on = Selection::parse("enabled = [\"recording\"]").unwrap();
        let states = states(&manifests, &on, "/nonexistent");
        let rec = states.iter().find(|s| s.id == "recording").unwrap();
        assert_eq!(rec.status, FeatureStatus::MissingDeps);
        let off = super::states(&manifests, &Selection::parse("disabled = [\"recording\"]").unwrap(), "/nonexistent");
        assert_eq!(off.iter().find(|s| s.id == "recording").unwrap().status, FeatureStatus::Off);
    }

    fn with_slot(id: &str, host: &str) -> Manifest {
        let text = format!(
            "id = \"{id}\"\nname = \"x\"\ndescription = \"x\"\ndefault = true\n[[slot]]\nhost = \"{host}\"\nview = \"{id}.view\"\n"
        );
        toml::from_str(&text).unwrap()
    }

    #[test]
    fn a_feature_whose_hosts_are_all_gone_has_no_host() {
        let manifests = [
            with_slot("bar", "bar.center"),
            with_slot("pill", "bar.left"),
            with_slot("stray", "nowhere"),
            with_slot("capsule", "shell.overlays"),
        ];
        let status = |states: &[FeatureState], id: &str| states.iter().find(|s| s.id == id).unwrap().status;

        let all_on = states(&manifests, &Selection::default(), "/nonexistent");
        assert_eq!(status(&all_on, "pill"), FeatureStatus::Ready);
        assert_eq!(status(&all_on, "stray"), FeatureStatus::NoHost, "an unknown host never shows it");
        assert_eq!(all_on.iter().find(|s| s.id == "pill").unwrap().slots, manifests[1].slots);

        let bar_off = states(&manifests, &Selection::parse("disabled = [\"bar\"]").unwrap(), "/nonexistent");
        assert_eq!(status(&bar_off, "pill"), FeatureStatus::NoHost, "the bar it goes in is off");
        assert_eq!(status(&bar_off, "bar"), FeatureStatus::Off);
        assert_eq!(status(&bar_off, "capsule"), FeatureStatus::Ready, "the shell draws overlays without the bar");
    }
}
