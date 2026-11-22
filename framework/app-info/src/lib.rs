//! An app as Nova's messages carry it ([`App`]): plain data, so api crates
//! and fronts can name apps without the index that finds them
//! (`nova-apps`, which builds these from desktop entries).

use serde::{Deserialize, Serialize};

/// An app from its desktop entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct App {
    /// Its desktop file id, like `org.mozilla.firefox.desktop`.
    pub id: String,
    /// Its name in the user's language.
    pub name: String,
    /// What kind of app it is or what it's for (its generic name or comment).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Its icon: an absolute path to an image, found in the icon theme
    /// when its entry names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}
