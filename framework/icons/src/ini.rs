use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

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

    pub fn read(file: &Path) -> Option<Self> {
        let mut text = String::new();
        std::fs::File::open(file).ok()?.take(MAX_BYTES).read_to_string(&mut text).ok()?;
        Some(Self::parse(&text))
    }

    pub fn get(&self, group: &str, key: &str) -> Option<&str> {
        self.groups.get(group)?.get(key).map(String::as_str).filter(|value| !value.is_empty())
    }

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
