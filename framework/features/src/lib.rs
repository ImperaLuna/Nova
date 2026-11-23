pub mod gpus;

pub use nova_daemon_info::Slot;
use nova_daemon_info::{FeatureState, FeatureStatus, UiProgram};

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub const DISTROS: &[&str] = &["nix", "arch", "fedora", "debian"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    pub name: &'static str,
    pub owner: Option<&'static str>,
    pub views: Views,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Views {
    Ui,
    Daemon,
}

pub const HOSTS: &[Host] = &[
    Host { name: "bar.left", owner: Some("bar"), views: Views::Ui },
    Host { name: "bar.center", owner: Some("bar"), views: Views::Ui },
    Host { name: "bar.right", owner: Some("bar"), views: Views::Ui },
    Host { name: "right-pill.buttons", owner: Some("bar"), views: Views::Ui },
    Host { name: "right-pill.tray", owner: Some("bar"), views: Views::Ui },
    Host { name: "central-pill.parts", owner: Some("bar"), views: Views::Ui },
    Host { name: "stage.panels", owner: None, views: Views::Ui },
    Host { name: "shell.overlays", owner: None, views: Views::Ui },
    Host { name: "quick-settings.tiles", owner: Some("quick-settings"), views: Views::Ui },
    Host { name: "quick-settings.sliders", owner: Some("quick-settings"), views: Views::Ui },
    Host { name: "quick-settings.cards", owner: Some("quick-settings"), views: Views::Ui },
    Host { name: "quick-settings.fill", owner: Some("quick-settings"), views: Views::Ui },
    Host { name: "quick-settings.foot", owner: Some("quick-settings"), views: Views::Ui },
    Host { name: "quick-settings.pages", owner: Some("quick-settings"), views: Views::Ui },
    Host { name: "launcher.providers", owner: Some("launcher"), views: Views::Daemon },
];

pub fn host(name: &str) -> Option<&'static Host> {
    HOSTS.iter().find(|host| host.name == name)
}

pub fn has_no_host(feature: &FeatureState, all: &[FeatureState]) -> bool {
    let ready = |id: &str| all.iter().any(|other| other.id == id && other.status == FeatureStatus::Ready);
    feature.status == FeatureStatus::Ready
        && !feature.slots.is_empty()
        && !feature.slots.iter().any(|slot| host(&slot.host).is_some_and(|host| host.owner.is_none_or(ready)))
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
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub daemon: Option<String>,
    #[serde(default)]
    pub modes: Vec<String>,
    #[serde(default, rename = "default-mode")]
    pub default_mode: Option<String>,
    #[serde(default)]
    pub variants: BTreeMap<String, Variant>,
    #[serde(default)]
    pub check: Check,
    #[serde(default, rename = "slot")]
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Variant {
    #[serde(default)]
    pub when: Option<When>,
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
    pub fn variant_for(&self, gpu: Option<Gpu>) -> Option<&Variant> {
        self.variants
            .iter()
            .filter(|(name, _)| name.as_str() != "default")
            .find(|(_, v)| v.when.as_ref().is_some_and(|w| w.gpu.is_some() && w.gpu == gpu))
            .map(|(_, v)| v)
            .or_else(|| self.variants.get("default"))
    }

    pub fn packages(&self, distro: &str, gpu: Option<Gpu>) -> &[String] {
        [self.variant_for(gpu), self.variants.get("default")]
            .into_iter()
            .flatten()
            .find_map(|variant| variant.packages.get(distro))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn has_ui(&self) -> bool {
        self.slots.iter().any(|slot| host(&slot.host).is_none_or(|host| host.views == Views::Ui))
    }

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
        for (index, mode) in self.modes.iter().enumerate() {
            if !kebab(mode) {
                return Err(format!("mode \"{mode}\" must be lowercase kebab-case"));
            }
            if self.modes[..index].contains(mode) {
                return Err(format!("mode \"{mode}\" is listed twice"));
            }
        }
        match &self.default_mode {
            None if !self.modes.is_empty() => return Err("modes need a `default-mode`".into()),
            Some(mode) if !self.modes.contains(mode) => {
                return Err(format!("default-mode \"{mode}\" isn't one of its modes"));
            }
            _ => {}
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

pub fn default_dir() -> PathBuf {
    match std::env::var_os("NOVA_FEATURES_DIR") {
        Some(dir) => dir.into(),
        None => nova_paths::installed_or_checkout("features"),
    }
}

pub fn load(dir: &Path) -> Result<Manifest, Error> {
    let path = dir.join("feature.toml");
    let err = |message: String| Error { path: path.clone(), message };
    let text = std::fs::read_to_string(&path).map_err(|e| err(e.to_string()))?;
    let manifest: Manifest = toml::from_str(&text).map_err(|e| err(e.to_string()))?;
    let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    manifest.validate(dir_name).map_err(err)?;
    Ok(manifest)
}

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

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(default)]
    pub ui: Option<String>,
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub disabled: Vec<String>,
    #[serde(default)]
    pub modes: BTreeMap<String, String>,
}

impl Selection {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn ui_problem(&self) -> Option<String> {
        let name = self.ui.as_deref()?;
        match UiProgram::named(name) {
            Some(UiProgram::Gpui) => None,
            Some(UiProgram::Quickshell) => Some(
                "features.toml picks the Quickshell UI, which Nova no longer has, so nova-ui runs: remove `ui` there"
                    .into(),
            ),
            None => Some(format!("features.toml picks a \"{name}\" UI, which Nova doesn't have, so nova-ui runs")),
        }
    }

    pub fn is_on(&self, manifest: &Manifest) -> bool {
        if self.disabled.contains(&manifest.id) {
            return false;
        }
        manifest.default || self.enabled.contains(&manifest.id)
    }

    pub fn mode<'a>(&self, manifest: &'a Manifest) -> Option<&'a str> {
        let picked = self.modes.get(&manifest.id);
        manifest.modes.iter().find(|mode| Some(*mode) == picked).or(manifest.default_mode.as_ref()).map(String::as_str)
    }

    pub fn unknown_modes<'a>(&'a self, manifests: &[Manifest]) -> Vec<(&'a str, &'a str)> {
        self.modes
            .iter()
            .filter(|(id, mode)| manifests.iter().any(|manifest| &manifest.id == *id && !manifest.modes.contains(mode)))
            .map(|(id, mode)| (id.as_str(), mode.as_str()))
            .collect()
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
    fn a_ui_other_than_nova_ui_only_warns() {
        let problem = |text: &str| Selection::parse(text).unwrap().ui_problem();
        assert_eq!(problem(""), None);
        assert_eq!(problem("ui = \"gpui\"\ndisabled = [\"bar\"]"), None);
        assert!(problem("ui = \"quickshell\"").is_some_and(|problem| problem.contains("no longer")));
        assert!(problem("ui = \"GPUI\"").is_some_and(|problem| problem.contains("\"GPUI\"")));
    }

    #[test]
    fn every_manifest_in_the_repo_is_valid() {
        let all = load_all(&repo_features()).unwrap_or_else(|e| panic!("{e}"));
        assert!(all.iter().any(|m| m.id == "bar"));
    }

    #[test]
    fn gpu_picks_the_matching_variant() {
        let m = manifest(RECORDING);
        assert_eq!(m.packages("arch", Some(Gpu::Nvidia)), ["gpu-screen-recorder"]);
        assert_eq!(m.packages("arch", Some(Gpu::Amd)), ["wf-recorder"]);
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
        let modes = |extra: &str| format!("id = \"a\"\nname = \"x\"\ndescription = \"x\"\n{extra}");
        let cases = cases.into_iter().map(|(text, dir, expected)| (text.to_string(), dir, expected)).chain([
            (modes("modes = [\"Big\"]\ndefault-mode = \"Big\""), "a", "kebab-case"),
            (modes("modes = [\"lite\", \"lite\"]\ndefault-mode = \"lite\""), "a", "twice"),
            (modes("modes = [\"lite\"]"), "a", "need a `default-mode`"),
            (modes("modes = [\"lite\"]\ndefault-mode = \"full\""), "a", "isn't one of its modes"),
            (modes("default-mode = \"full\""), "a", "isn't one of its modes"),
        ]);
        for (text, dir, expected) in cases {
            let err = manifest(&text).validate(dir).unwrap_err();
            assert!(err.contains(expected), "{err:?} should mention {expected:?}");
        }
    }

    #[test]
    fn selection_respects_default_and_choices() {
        let bar = manifest("id = \"bar\"\nname = \"x\"\ndescription = \"x\"\ndefault = true");
        let rec = manifest(RECORDING);
        let none = Selection::default();
        assert!(none.is_on(&bar) && !none.is_on(&rec));

        let sel = Selection::parse("enabled = [\"recording\"]\ndisabled = [\"bar\"]").unwrap();
        assert!(!sel.is_on(&bar), "every feature can be turned off, the bar too");
        assert!(sel.is_on(&rec));
    }

    #[test]
    fn a_mode_is_the_one_picked_else_the_default() {
        let files = manifest(
            "id = \"files\"\nname = \"x\"\ndescription = \"x\"\nmodes = [\"lite\", \"full\"]\ndefault-mode = \"lite\"",
        );
        let rec = manifest(RECORDING);
        assert_eq!(Selection::default().mode(&files), Some("lite"));
        assert_eq!(Selection::default().mode(&rec), None, "no modes, no mode");
        let picked = Selection::parse("[modes]\nfiles = \"full\"\nrecording = \"fast\"").unwrap();
        assert_eq!(picked.mode(&files), Some("full"));
        assert_eq!(picked.mode(&rec), None);
        let typo = Selection::parse("[modes]\nfiles = \"ful\"\ngone = \"x\"").unwrap();
        assert_eq!(typo.mode(&files), Some("lite"), "a mode it doesn't have counts as none");
        assert_eq!(typo.unknown_modes(&[files, rec]), [("files", "ful")]);
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

    #[test]
    fn a_host_s_owner_is_a_feature() {
        let ids: Vec<String> = load_all(&repo_features()).unwrap().into_iter().map(|m| m.id).collect();
        for host in HOSTS {
            if let Some(owner) = host.owner {
                assert!(ids.iter().any(|id| id == owner), "{} is owned by unknown feature {owner}", host.name);
            }
        }
    }

    #[test]
    fn only_slots_with_ui_views_need_the_ui() {
        let provider = "id = \"calc\"\nname = \"x\"\ndescription = \"x\"\n[[slot]]\nhost = \"launcher.providers\"\nview = \"calc\"";
        assert!(!toml::from_str::<Manifest>(provider).unwrap().has_ui());
        let pill =
            "id = \"calc\"\nname = \"x\"\ndescription = \"x\"\n[[slot]]\nhost = \"bar.left\"\nview = \"calc.pill\"";
        assert!(toml::from_str::<Manifest>(pill).unwrap().has_ui());
    }

    #[test]
    fn every_slot_names_a_known_host() {
        for manifest in load_all(&repo_features()).unwrap() {
            for slot in &manifest.slots {
                assert!(host(&slot.host).is_some(), "{}: unknown host {}", manifest.id, slot.host);
            }
        }
    }

    fn state(id: &str, status: FeatureStatus, hosts: &[&str]) -> FeatureState {
        let slots =
            hosts.iter().map(|host| Slot { host: host.to_string(), view: format!("{id}.view"), order: 0 }).collect();
        FeatureState { id: id.into(), status, missing: vec![], slots, mode: None }
    }

    #[test]
    fn a_feature_whose_hosts_are_all_gone_has_no_host() {
        let bar = state("bar", FeatureStatus::Ready, &["bar.center"]);
        let pill = state("pill", FeatureStatus::Ready, &["bar.left"]);
        let stray = state("stray", FeatureStatus::Ready, &["nowhere"]);
        let popups = state("popups", FeatureStatus::Ready, &[]);
        let capsule = state("capsule", FeatureStatus::Ready, &["shell.overlays"]);
        let all = [bar.clone(), pill.clone(), stray.clone(), popups.clone(), capsule.clone()];
        assert!(!has_no_host(&pill, &all));
        assert!(has_no_host(&stray, &all), "an unknown host never shows it");
        assert!(!has_no_host(&popups, &all), "no slots: it shows elsewhere");

        let bar_off = state("bar", FeatureStatus::Off, &["bar.center"]);
        let all = [bar_off.clone(), pill.clone(), capsule.clone()];
        assert!(has_no_host(&pill, &all), "the bar it goes in is off");
        assert!(!has_no_host(&bar_off, &all), "off is its own state");
        assert!(!has_no_host(&capsule, &all), "the shell draws overlays without the bar");
    }
}
