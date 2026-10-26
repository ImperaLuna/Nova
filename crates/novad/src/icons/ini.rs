//! The INI-style files icon themes are described and chosen in: an icon
//! theme's `index.theme`, GTK's `settings.ini`, qt6ct's config and KDE's
//! `kdeglobals`. `[Group]` lines, `key=value` lines, and `#` or `;` comments.
//! A key seen twice in one group keeps its first value.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// These files are a few kilobytes (Papirus's `index.theme` is the biggest
/// at about 40); anything far bigger isn't one.
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Default)]
pub struct Ini {
    groups: HashMap<String, HashMap<String, String>>,
}

impl Ini {
    pub fn parse(text: &str) -> Self {
        let mut ini = Self::default();
        let mut group = String::new();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with(['#', ';']) {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|line| line.strip_suffix(']')) {
                group = name.trim().to_string();
            } else if let Some((key, value)) = line.split_once('=') {
                let keys = ini.groups.entry(group.clone()).or_default();
                keys.entry(key.trim().to_string()).or_insert_with(|| value.trim().to_string());
            }
        }
        ini
    }

    /// `file` parsed, or `None` when it can't be read.
    pub fn read(file: &Path) -> Option<Self> {
        let mut text = String::new();
        std::fs::File::open(file).ok()?.take(MAX_BYTES).read_to_string(&mut text).ok()?;
        Some(Self::parse(&text))
    }

    /// `key`'s value in `group`; an empty one is none.
    pub fn get(&self, group: &str, key: &str) -> Option<&str> {
        self.groups.get(group)?.get(key).map(String::as_str).filter(|value| !value.is_empty())
    }

    /// `key`'s comma-separated list in `group`, without empty items.
    pub fn list(&self, group: &str, key: &str) -> Vec<&str> {
        self.get(group, key)
            .map(|value| value.split(',').map(str::trim).filter(|item| !item.is_empty()).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_groups_keys_and_lists() {
        let ini = Ini::parse(
            "# a comment\n[Icon Theme]\nName = Papirus\nInherits=breeze, hicolor,\nName=Ignored\n\n[16x16/apps]\n; also a comment\nSize=16\nEmpty=\n",
        );
        assert_eq!(ini.get("Icon Theme", "Name"), Some("Papirus"));
        assert_eq!(ini.list("Icon Theme", "Inherits"), ["breeze", "hicolor"]);
        assert_eq!(ini.get("16x16/apps", "Size"), Some("16"));
        assert_eq!(ini.get("16x16/apps", "Empty"), None);
        assert_eq!(ini.get("Icon Theme", "Size"), None);
        assert!(ini.list("Nowhere", "Inherits").is_empty());
    }
}
