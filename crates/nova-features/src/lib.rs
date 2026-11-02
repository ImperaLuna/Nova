//! Feature manifests (`features/<id>/feature.toml`).
//!
//! A manifest is the single source of truth for one feature. Nix reads it
//! with `builtins.fromTOML`, `nova setup` reads it to pick packages on other
//! distros, and `novad` reads it to decide what the UI may load.
//!
//! ```toml
//! id = "recording"
//! name = "Screen recording"
//! description = "Record the screen or a region."
//! default = false
//!
//! [variants.nvidia]
//! when = { gpu = "nvidia" }
//! nix = ["gpu-screen-recorder"]
//! arch = ["gpu-screen-recorder"]
//!
//! [variants.default]
//! nix = ["wf-recorder"]
//! arch = ["wf-recorder"]
//!
//! [check]
//! binaries = ["gpu-screen-recorder|wf-recorder"]
//!
//! [[slot]]
//! host = "bar.right"
//! view = "recording.pill"
//! order = 10
//! ```
//!
//! Each `[[slot]]` is a place its UI goes: a view (an id the UI maps to its
//! component in ui/slots/Views.qml) in one of the [`HOSTS`], sorted there by
//! `order`. novad sends them with the feature's state, so a new feature
//! plugs into the bar from its own manifest instead of the shell's files.

pub mod gpus;

pub use nova_protocol::Slot;
use nova_protocol::{FeatureState, FeatureStatus};

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Package sets a variant may list. `nix` holds nixpkgs attribute names.
pub const DISTROS: &[&str] = &["nix", "arch", "fedora", "debian"];

/// A place in the UI that slots fill, and the feature that draws it. The
/// UI's names for them are the `HOST_*` constants in ui/slots/slots.js.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    pub name: &'static str,
    /// The feature whose UI draws the host: its slots have nowhere to show
    /// while that feature isn't ready.
    pub owner: &'static str,
}

/// Every host a slot may name. A slot naming another host is never filled.
pub const HOSTS: &[Host] = &[
    // The bar's rows of pills: left from the left edge, center around the
    // middle, right from the right edge.
    Host { name: "bar.left", owner: "core" },
    Host { name: "bar.center", owner: "core" },
    Host { name: "bar.right", owner: "core" },
    // The buttons at the right pill's end (and on quick settings, in the
    // same place).
    Host { name: "right-pill.buttons", owner: "core" },
    // Panels drawn on the stage or its panel surfaces that aren't a pill
    // of their own, like the launcher.
    Host { name: "stage.panels", owner: "core" },
];

/// The host named `name`, if there is one.
pub fn host(name: &str) -> Option<&'static Host> {
    HOSTS.iter().find(|host| host.name == name)
}

/// Whether `feature` is ready but has slots and none of them can show:
/// each names a host that doesn't exist or whose owner (in `all`) isn't
/// ready. `nova doctor` warns about it; the feature still runs, since it
/// may show elsewhere too (pop-ups, keybinds).
pub fn has_no_host(feature: &FeatureState, all: &[FeatureState]) -> bool {
    let ready = |id: &str| all.iter().any(|other| other.id == id && other.status == FeatureStatus::Ready);
    feature.status == FeatureStatus::Ready
        && !feature.slots.is_empty()
        && !feature.slots.iter().any(|slot| host(&slot.host).is_some_and(|host| ready(host.owner)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gpu {
    Nvidia,
    Amd,
    Intel,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Turned on when the user hasn't said otherwise.
    #[serde(default)]
    pub default: bool,
    /// Can't be turned off (the bar, launcher, notifications).
    #[serde(default)]
    pub required: bool,
    /// Names the feature's daemon side in novad (`novad::modules`).
    #[serde(default)]
    pub daemon: Option<String>,
    #[serde(default)]
    pub variants: BTreeMap<String, Variant>,
    #[serde(default)]
    pub check: Check,
    /// Where its UI goes (`[[slot]]`).
    #[serde(default, rename = "slot")]
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Variant {
    #[serde(default)]
    pub when: Option<When>,
    /// Distro name to package names, e.g. `arch = ["wf-recorder"]`.
    #[serde(flatten)]
    pub packages: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct When {
    pub gpu: Option<Gpu>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// Each entry must resolve on `PATH`. `a|b` means either one.
    #[serde(default)]
    pub binaries: Vec<String>,
}

#[derive(Debug)]
pub struct Error {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for Error {}

impl Manifest {
    /// The variant that applies on this machine: the first one whose `when`
    /// matches, otherwise `default`.
    pub fn variant_for(&self, gpu: Option<Gpu>) -> Option<&Variant> {
        self.variants
            .iter()
            .filter(|(name, _)| name.as_str() != "default")
            .find(|(_, v)| v.when.as_ref().is_some_and(|w| w.gpu.is_some() && w.gpu == gpu))
            .map(|(_, v)| v)
            .or_else(|| self.variants.get("default"))
    }

    /// Packages to install for `distro` (one of [`DISTROS`]): the matching
    /// variant's, else `default`'s when that variant doesn't list `distro`
    /// (gpu-screen-recorder isn't packaged everywhere).
    pub fn packages(&self, distro: &str, gpu: Option<Gpu>) -> &[String] {
        [self.variant_for(gpu), self.variants.get("default")]
            .into_iter()
            .flatten()
            .find_map(|variant| variant.packages.get(distro))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Entries of `check.binaries` with no match on `PATH`.
    pub fn missing_binaries(&self, path_var: &str) -> Vec<String> {
        self.check
            .binaries
            .iter()
            .filter(|entry| !entry.split('|').any(|bin| on_path(bin, path_var)))
            .cloned()
            .collect()
    }

    fn validate(&self, dir_name: &str) -> Result<(), String> {
        if self.id != dir_name {
            return Err(format!("id \"{}\" must match its folder \"{dir_name}\"", self.id));
        }
        let kebab = |s: &str| {
            !s.is_empty()
                && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && !s.starts_with('-')
                && !s.ends_with('-')
        };
        if !kebab(&self.id) {
            return Err(format!("id \"{}\" must be lowercase kebab-case", self.id));
        }
        if self.required && !self.default {
            return Err("a required feature must also be default = true".into());
        }
        if !self.variants.is_empty() && !self.variants.contains_key("default") {
            return Err("variants need a [variants.default] fallback".into());
        }
        for (name, variant) in &self.variants {
            if name == "default" && variant.when.is_some() {
                return Err("[variants.default] can't have a `when`".into());
            }
            if name != "default" && variant.when.as_ref().is_none_or(|w| w.gpu.is_none()) {
                return Err(format!("[variants.{name}] needs a `when` condition"));
            }
            if let Some(bad) = variant.packages.keys().find(|d| !DISTROS.contains(&d.as_str())) {
                return Err(format!(
                    "[variants.{name}] lists unknown distro \"{bad}\" (known: {})",
                    DISTROS.join(", ")
                ));
            }
        }
        for (index, slot) in self.slots.iter().enumerate() {
            let well_formed = slot.view.split('.').all(kebab);
            if !well_formed {
                return Err(format!("[[slot]] view \"{}\" must be dot-separated kebab-case", slot.view));
            }
            if self.slots[..index].iter().any(|other| other.host == slot.host && other.view == slot.view) {
                return Err(format!("[[slot]] {} in {} is listed twice", slot.view, slot.host));
            }
        }
        Ok(())
    }
}

fn on_path(bin: &str, path_var: &str) -> bool {
    nova_paths::find_program(bin, path_var).is_some()
}

/// `$NOVA_FEATURES_DIR`, else the installed set, else `./features` for a checkout.
pub fn default_dir() -> PathBuf {
    match std::env::var_os("NOVA_FEATURES_DIR") {
        Some(dir) => dir.into(),
        None => nova_paths::installed_or_checkout("features"),
    }
}

/// Parses and validates `<dir>/feature.toml`.
pub fn load(dir: &Path) -> Result<Manifest, Error> {
    let path = dir.join("feature.toml");
    let err = |message: String| Error { path: path.clone(), message };
    let text = std::fs::read_to_string(&path).map_err(|e| err(e.to_string()))?;
    let manifest: Manifest = toml::from_str(&text).map_err(|e| err(e.to_string()))?;
    let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    manifest.validate(dir_name).map_err(err)?;
    Ok(manifest)
}

/// Loads every `<root>/*/feature.toml`, sorted by id.
pub fn load_all(root: &Path) -> Result<Vec<Manifest>, Error> {
    let entries = std::fs::read_dir(root).map_err(|e| Error { path: root.into(), message: e.to_string() })?;
    let mut manifests = Vec::new();
    for entry in entries.flatten() {
        if entry.path().join("feature.toml").is_file() {
            manifests.push(load(&entry.path())?);
        }
    }
    manifests.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(manifests)
}

/// The user's choices from `features.toml`: `enabled = [...]`, `disabled = [...]`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub disabled: Vec<String>,
}

impl Selection {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn is_on(&self, manifest: &Manifest) -> bool {
        if manifest.required {
            return true;
        }
        if self.disabled.contains(&manifest.id) {
            return false;
        }
        manifest.default || self.enabled.contains(&manifest.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_features() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features")
    }

    fn manifest(text: &str) -> Manifest {
        toml::from_str(text).unwrap()
    }

    const RECORDING: &str = r#"
        id = "recording"
        name = "Screen recording"
        description = "Record the screen."
        [variants.nvidia]
        when = { gpu = "nvidia" }
        arch = ["gpu-screen-recorder"]
        [variants.default]
        arch = ["wf-recorder"]
        [check]
        binaries = ["gpu-screen-recorder|wf-recorder"]
    "#;

    #[test]
    fn every_manifest_in_the_repo_is_valid() {
        let all = load_all(&repo_features()).unwrap_or_else(|e| panic!("{e}"));
        assert!(all.iter().any(|m| m.id == "core" && m.required));
    }

    #[test]
    fn gpu_picks_the_matching_variant() {
        let m = manifest(RECORDING);
        assert_eq!(m.packages("arch", Some(Gpu::Nvidia)), ["gpu-screen-recorder"]);
        assert_eq!(m.packages("arch", Some(Gpu::Amd)), ["wf-recorder"]);
        // The NVIDIA variant lists no Fedora package: the default's applies.
        let partial = manifest(
            r#"
            id = "recording"
            name = "Screen recording"
            description = "Record the screen."
            [variants.nvidia]
            when = { gpu = "nvidia" }
            arch = ["gpu-screen-recorder"]
            [variants.default]
            fedora = ["wf-recorder"]
        "#,
        );
        assert_eq!(partial.packages("fedora", Some(Gpu::Nvidia)), ["wf-recorder"]);
        assert!(partial.packages("debian", Some(Gpu::Nvidia)).is_empty());
        assert_eq!(m.packages("arch", None), ["wf-recorder"]);
        assert!(m.packages("fedora", None).is_empty());
    }

    #[test]
    fn alternatives_count_as_present() {
        let dir = nova_test_support::TempDir::new("features");
        nova_test_support::write_program(dir.path(), "wf-recorder", "exit 0");
        let m = manifest(RECORDING);
        assert!(m.missing_binaries(&dir.path_var()).is_empty());
        assert_eq!(m.missing_binaries("/nonexistent"), ["gpu-screen-recorder|wf-recorder"]);
    }

    #[test]
    fn validation_catches_mistakes() {
        let cases = [
            ("id = \"Rec\"\nname = \"x\"\ndescription = \"x\"", "Rec", "kebab-case"),
            ("id = \"a\"\nname = \"x\"\ndescription = \"x\"", "b", "must match"),
            ("id = \"a\"\nname = \"x\"\ndescription = \"x\"\nrequired = true", "a", "default = true"),
            (
                "id = \"a\"\nname = \"x\"\ndescription = \"x\"\n[variants.nvidia]\nwhen = { gpu = \"nvidia\" }",
                "a",
                "fallback",
            ),
            (
                "id = \"a\"\nname = \"x\"\ndescription = \"x\"\n[variants.default]\nubuntu = [\"x\"]",
                "a",
                "unknown distro",
            ),
            (
                "id = \"a\"\nname = \"x\"\ndescription = \"x\"\n[variants.big]\n[variants.default]",
                "a",
                "needs a `when`",
            ),
            (
                "id = \"a\"\nname = \"x\"\ndescription = \"x\"\n[[slot]]\nhost = \"bar.left\"\nview = \"a/pill\"",
                "a",
                "kebab-case",
            ),
            (
                "id = \"a\"\nname = \"x\"\ndescription = \"x\"\n[[slot]]\nhost = \"bar.left\"\nview = \"a.pill\"\n[[slot]]\nhost = \"bar.left\"\nview = \"a.pill\"",
                "a",
                "twice",
            ),
        ];
        for (text, dir, expected) in cases {
            let err = manifest(text).validate(dir).unwrap_err();
            assert!(err.contains(expected), "{err:?} should mention {expected:?}");
        }
    }

    #[test]
    fn selection_respects_required_default_and_choices() {
        let core = manifest("id = \"core\"\nname = \"x\"\ndescription = \"x\"\ndefault = true\nrequired = true");
        let rec = manifest(RECORDING);
        let none = Selection::default();
        assert!(none.is_on(&core) && !none.is_on(&rec));

        let sel = Selection::parse("enabled = [\"recording\"]\ndisabled = [\"core\"]").unwrap();
        assert!(sel.is_on(&core), "required features can't be disabled");
        assert!(sel.is_on(&rec));
    }

    #[test]
    fn slots_parse_with_their_order() {
        let m = manifest(
            "id = \"a\"\nname = \"x\"\ndescription = \"x\"\n[[slot]]\nhost = \"bar.left\"\nview = \"a.pill\"\norder = 15\n[[slot]]\nhost = \"stage.panels\"\nview = \"a.panel\"",
        );
        let expected = [
            Slot { host: "bar.left".into(), view: "a.pill".into(), order: 15 },
            Slot { host: "stage.panels".into(), view: "a.panel".into(), order: 0 },
        ];
        assert_eq!(m.slots, expected);
        let unknown = "id = \"a\"\nname = \"x\"\ndescription = \"x\"\n[[slot]]\nhost = \"bar.left\"\nview = \"a.pill\"\nwhere = 1";
        assert!(toml::from_str::<Manifest>(unknown).is_err(), "unknown slot fields are rejected");
    }

    /// The quoted strings of `prefix` lines in a UI file, like
    /// `var HOST_BAR_LEFT = "bar.left";` or `"media.pill": "../media/X.qml",`.
    fn js_strings(file: &str, prefix: &str) -> Vec<Vec<String>> {
        let text = std::fs::read_to_string(repo_features().join("../ui").join(file)).unwrap();
        text.lines()
            .map(str::trim)
            .filter(|line| line.starts_with(prefix))
            .map(|line| line.split('"').skip(1).step_by(2).map(String::from).collect())
            .collect()
    }

    #[test]
    fn the_ui_names_the_same_hosts() {
        let mut ui: Vec<String> = js_strings("slots/slots.js", "var HOST_").into_iter().flatten().collect();
        let mut known: Vec<String> = HOSTS.iter().map(|host| host.name.to_string()).collect();
        ui.sort();
        known.sort();
        assert_eq!(ui, known, "ui/slots/slots.js and HOSTS differ");
        let ids: Vec<String> = load_all(&repo_features()).unwrap().into_iter().map(|m| m.id).collect();
        for host in HOSTS {
            assert!(ids.iter().any(|id| id == host.owner), "{} is owned by unknown feature {}", host.name, host.owner);
        }
    }

    #[test]
    fn every_slot_names_a_known_host_and_a_registered_view() {
        let views = js_strings("slots/Views.qml", "\"");
        let imports = js_strings("slots/Views.qml", "import \"../");
        let slots = repo_features().join("../ui/slots");
        let whole_ui = repo_features().join("../ui/shell.qml").is_file();
        for manifest in load_all(&repo_features()).unwrap() {
            for slot in &manifest.slots {
                assert!(host(&slot.host).is_some(), "{}: unknown host {}", manifest.id, slot.host);
                let file = views
                    .iter()
                    .find(|entry| entry.first() == Some(&slot.view))
                    .and_then(|entry| entry.get(1))
                    .unwrap_or_else(|| panic!("{}: view {} isn't in ui/slots/Views.qml", manifest.id, slot.view));
                // The Nix build's source has ui/slots but not the views.
                if whole_ui {
                    assert!(slots.join(file).is_file(), "{}: {file} doesn't exist next to ui/slots", slot.view);
                }
                let folder = file.split('/').nth(1).unwrap_or_default();
                assert!(
                    imports.iter().any(|import| import.first().is_some_and(|dir| dir == &format!("../{folder}"))),
                    "{}: ui/slots/Views.qml must import \"../{folder}\" for Quickshell to load it",
                    slot.view
                );
            }
        }
    }

    fn state(id: &str, status: FeatureStatus, hosts: &[&str]) -> FeatureState {
        let slots =
            hosts.iter().map(|host| Slot { host: host.to_string(), view: format!("{id}.view"), order: 0 }).collect();
        FeatureState { id: id.into(), status, missing: vec![], slots }
    }

    #[test]
    fn a_feature_whose_hosts_are_all_gone_has_no_host() {
        let core = state("core", FeatureStatus::Ready, &["bar.center"]);
        let pill = state("pill", FeatureStatus::Ready, &["bar.left"]);
        let stray = state("stray", FeatureStatus::Ready, &["nowhere"]);
        let popups = state("popups", FeatureStatus::Ready, &[]);
        let all = [core.clone(), pill.clone(), stray.clone(), popups.clone()];
        assert!(!has_no_host(&pill, &all));
        assert!(has_no_host(&stray, &all), "an unknown host never shows it");
        assert!(!has_no_host(&popups, &all), "no slots: it shows elsewhere");

        let core_off = state("core", FeatureStatus::Off, &["bar.center"]);
        let all = [core_off.clone(), pill.clone()];
        assert!(has_no_host(&pill, &all), "the bar it goes in is off");
        assert!(!has_no_host(&core_off, &all), "off is its own state");
    }
}
