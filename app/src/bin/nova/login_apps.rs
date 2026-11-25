use nova_tray_api::TrayItem;
use std::path::Path;

pub const AUTOSTART_DIR: &str = "~/.config/autostart";
const PREFIXES: [&str; 4] = ["env", "uwsm", "app", "--"];

#[derive(Debug, Clone, PartialEq)]
pub struct StartupCommand {
    pub source: &'static str,
    pub program: String,
}

pub fn read(config_home: &Path) -> Vec<StartupCommand> {
    let niri = std::env::var_os("NIRI_CONFIG").map(Into::into).unwrap_or_else(|| config_home.join("niri/config.kdl"));
    let mut commands = Vec::new();
    if let Ok(text) = std::fs::read_to_string(niri) {
        commands.extend(niri_commands(&text));
    }
    if let Ok(text) = std::fs::read_to_string(config_home.join("hypr/hyprland.conf")) {
        commands.extend(hyprland_commands(&text));
    }
    commands
}

fn niri_commands(text: &str) -> Vec<StartupCommand> {
    const SOURCE: &str = "niri's spawn-at-startup";
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let words = match line.strip_prefix("spawn-at-startup ") {
                Some(rest) => kdl_strings(rest),
                None => shell_words(kdl_strings(line.strip_prefix("spawn-sh-at-startup ")?).first()?),
            };
            Some(StartupCommand { source: SOURCE, program: program(&words)? })
        })
        .collect()
}

fn hyprland_commands(text: &str) -> Vec<StartupCommand> {
    const SOURCE: &str = "Hyprland's exec-once";
    text.lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix("exec-once")?.trim_start().strip_prefix('=')?;
            let command = rest.split_once(" #").map_or(rest, |(command, _)| command);
            Some(StartupCommand { source: SOURCE, program: program(&shell_words(command))? })
        })
        .collect()
}

fn kdl_strings(text: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut chars = text.trim_start().chars().peekable();
    while chars.next_if_eq(&'"').is_some() {
        let mut string = String::new();
        loop {
            match chars.next() {
                Some('"') | None => break,
                Some('\\') => string.extend(chars.next()),
                Some(c) => string.push(c),
            }
        }
        strings.push(string);
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
    }
    strings
}

fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for c in line.chars() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), _) if c == open => quote = None,
            (None, c) if c.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

fn program(words: &[String]) -> Option<String> {
    let word = words
        .iter()
        .find(|word| !PREFIXES.contains(&word.as_str()) && !word.starts_with('-') && !word.contains('='))?;
    Some(word.rsplit('/').next().unwrap_or(word).to_string())
}

pub fn tray_apps<'a>(commands: &'a [StartupCommand], tray: &'a [TrayItem]) -> Vec<(&'a str, &'a str)> {
    let mut found = Vec::new();
    for item in tray {
        let mut names: Vec<String> = item.id.split('_').next().map(str::to_lowercase).into_iter().collect();
        if let Some(app) = &item.app {
            let id = app.id.strip_suffix(".desktop").unwrap_or(&app.id).to_lowercase();
            names.extend(id.rsplit('.').next().map(str::to_string));
            names.push(id);
            names.push(app.name.to_lowercase());
        }
        names.retain(|name| !name.is_empty());
        let name = item.app.as_ref().map_or(item.id.as_str(), |app| app.name.as_str());
        for command in commands {
            if names.contains(&command.program.to_lowercase()) && !found.contains(&(name, command.source)) {
                found.push((name, command.source));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_app_info::App;

    fn programs(commands: Vec<StartupCommand>) -> Vec<(String, &'static str)> {
        commands.into_iter().map(|command| (command.program, command.source)).collect()
    }

    #[test]
    fn niri_lines_name_their_programs() {
        let text = r#"
spawn-at-startup "Discord" "--start-minimized"
    spawn-at-startup "env" "ELECTRON_OZONE_PLATFORM_HINT=x11" "ferdium" // the x11 one
spawn-sh-at-startup "FOO=1 /usr/bin/steam -silent"
// spawn-at-startup "commented"
spawn "not-at-startup"
"#;
        let niri = "niri's spawn-at-startup";
        assert_eq!(
            programs(niri_commands(text)),
            [("Discord".into(), niri), ("ferdium".into(), niri), ("steam".into(), niri)]
        );
    }

    #[test]
    fn hyprland_lines_name_their_programs() {
        let text = "exec-once = nova\nexec-once=uwsm app -- 'discord' --start-minimized # tray\n#exec-once = old\nexec = waybar\n";
        let hyprland = "Hyprland's exec-once";
        assert_eq!(programs(hyprland_commands(text)), [("nova".into(), hyprland), ("discord".into(), hyprland)]);
    }

    fn item(id: &str, app: Option<(&str, &str)>) -> TrayItem {
        TrayItem {
            address: ":1.5/StatusNotifierItem".into(),
            id: id.into(),
            title: String::new(),
            app: app.map(|(id, name)| App { id: id.into(), name: name.into(), description: None, icon: None }),
            menu_only: false,
            icon: None,
            attention: false,
            has_menu: false,
            is_menu: false,
        }
    }

    #[test]
    fn only_tray_apps_are_named() {
        let niri = "niri's spawn-at-startup";
        let commands: Vec<StartupCommand> = ["Discord", "ferdium", "nova", "steam"]
            .map(|program| StartupCommand { source: niri, program: program.into() })
            .into();
        let tray = [
            item("discord_status_icon_1", Some(("discord.desktop", "Discord"))),
            item("Ferdium", None),
            item("steam", Some(("com.valvesoftware.Steam.desktop", "Steam"))),
            item("blueman", Some(("blueman-manager.desktop", "Bluetooth Manager"))),
        ];
        assert_eq!(tray_apps(&commands, &tray), [("Discord", niri), ("Ferdium", niri), ("Steam", niri)]);
    }
}
