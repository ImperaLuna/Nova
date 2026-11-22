//! The machine and its user as the quick settings header gets them
//! ([`Host`]): plain data, so api crates and fronts can name it without the
//! code that reads it (`nova-host`).

use serde::{Deserialize, Serialize};

/// The machine and its user, as the quick settings header shows them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
    /// The machine's host name.
    pub host: String,
    /// What to call the user: config.toml's `[quick-settings] name` ("" for
    /// none), else their login name.
    pub user: String,
    /// When the machine booted, in seconds since the Unix epoch.
    pub booted_at: i64,
    pub os: OperatingSystem,
}

/// The distribution, from os-release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatingSystem {
    /// `ID`, like `nixos` or `arch`; `linux` when unset.
    pub id: String,
    /// `ID_LIKE`: the distributions it derives from, closest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub id_like: Vec<String>,
    /// `NAME`, like `NixOS`.
    pub name: String,
    /// `LOGO`: the icon theme's file for the logo it names, when set and
    /// the theme has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
}
