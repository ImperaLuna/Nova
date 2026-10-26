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
//! ```

pub mod gpus;

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Package sets a variant may list. `nix` holds nixpkgs attribute names.
pub const DISTROS: &[&str] = &["nix", "arch", "fedora", "debian"];

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
    /// Cargo feature of `novad` that holds this feature's daemon side.
    #[serde(default)]
    pub daemon: Option<String>,
    #[serde(default)]
    pub variants: BTreeMap<String, Variant>,
    #[serde(default)]
    pub check: Check,
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
        Ok(())
    }
}

fn on_path(bin: &str, path_var: &str) -> bool {
    std::env::split_paths(path_var).any(|dir| dir.join(bin).is_file())
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
        let dir = std::env::temp_dir().join(format!("nova-features-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wf-recorder"), "").unwrap();
        let m = manifest(RECORDING);
        assert!(m.missing_binaries(dir.to_str().unwrap()).is_empty());
        assert_eq!(m.missing_binaries("/nonexistent"), ["gpu-screen-recorder|wf-recorder"]);
        std::fs::remove_dir_all(dir).unwrap();
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
}
