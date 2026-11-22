//! `nova ipc`: one raw request to novad, its result printed as JSON, for
//! scripts. The command and its arguments are checked against the types
//! novad reads (`nova-protocol`'s, or a moved feature's api crate's) before
//! anything is sent. `subscribe` instead prints the
//! events of its topics, one JSON object per line, until the command is
//! interrupted (watching a topic from a shell, like
//! `scripts/desktop/isolated-novad` does); novad going away is an error, so
//! a script can tell it from a normal end.

use crate::client::Client;
use nova_carrier::names::tagged_names_of;
use nova_protocol::Command;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

#[derive(clap::Args)]
pub struct IpcArgs {
    /// The command, as novad names it: `status`, `list_features`,
    /// `focus_workspace`...
    command: String,
    /// Its arguments as a JSON object, like `'{"id": 3}'`.
    args: Option<String>,
}

pub fn run(args: &IpcArgs, socket: &Path) -> Result<(), String> {
    let command = parse(&args.command, args.args.as_deref())?;
    let mut client = Client::reach(socket).map_err(|unreachable| unreachable.advice())?;
    let follows = matches!(command, IpcCommand::Shared(Command::Subscribe { .. }));
    let result = client.request(command)?;
    if !follows {
        println!("{result}");
        return Ok(());
    }
    client.follow()?;
    let mut stdout = std::io::stdout().lock();
    while let Some(event) = client.next_event()? {
        let line = serde_json::to_string(&event).map_err(|e| e.to_string())?;
        // A closed pipe (`| head`) ends the watch, quietly.
        if writeln!(stdout, "{line}").and_then(|()| stdout.flush()).is_err() {
            return Ok(());
        }
    }
    Err("novad closed the connection".into())
}

/// A command `nova ipc` sends: a shared one, or a moved feature's own (its
/// api crate's `Command`, one variant per feature, sorted by feature id).
#[derive(Debug, PartialEq, Serialize)]
#[serde(untagged)]
enum IpcCommand {
    Shared(Command),
    AiUsage(nova_ai_usage_api::Command),
    Launcher(nova_launcher_api::Command),
    Lock(nova_lock_api::Command),
    Media(nova_media_api::Command),
    Network(nova_network_api::Command),
    Notifications(nova_notifications_api::Command),
    Power(nova_power_api::Command),
    Resources(nova_resources_api::Command),
    Screenshot(nova_screenshot_api::Command),
    Tray(nova_tray_api::Command),
    Wallpaper(nova_wallpaper_api::Command),
}

/// The command `name` with `args`, as novad would read it off the socket.
fn parse(name: &str, args: Option<&str>) -> Result<IpcCommand, String> {
    let mut request = serde_json::json!({ "cmd": name });
    if let Some(args) = args {
        request["args"] = serde_json::from_str(args).map_err(|e| format!("the arguments aren't JSON: {e}"))?;
    }
    let read = if tagged_names_of::<nova_ai_usage_api::Command>("cmd").contains(&name) {
        serde_json::from_value(request).map(IpcCommand::AiUsage)
    } else if tagged_names_of::<nova_launcher_api::Command>("cmd").contains(&name) {
        serde_json::from_value(request).map(IpcCommand::Launcher)
    } else if tagged_names_of::<nova_lock_api::Command>("cmd").contains(&name) {
        serde_json::from_value(request).map(IpcCommand::Lock)
    } else if tagged_names_of::<nova_media_api::Command>("cmd").contains(&name) {
        serde_json::from_value(request).map(IpcCommand::Media)
    } else if tagged_names_of::<nova_network_api::Command>("cmd").contains(&name) {
        serde_json::from_value(request).map(IpcCommand::Network)
    } else if tagged_names_of::<nova_notifications_api::Command>("cmd").contains(&name) {
        serde_json::from_value(request).map(IpcCommand::Notifications)
    } else if tagged_names_of::<nova_power_api::Command>("cmd").contains(&name) {
        serde_json::from_value(request).map(IpcCommand::Power)
    } else if tagged_names_of::<nova_resources_api::Command>("cmd").contains(&name) {
        serde_json::from_value(request).map(IpcCommand::Resources)
    } else if tagged_names_of::<nova_screenshot_api::Command>("cmd").contains(&name) {
        serde_json::from_value(request).map(IpcCommand::Screenshot)
    } else if tagged_names_of::<nova_tray_api::Command>("cmd").contains(&name) {
        serde_json::from_value(request).map(IpcCommand::Tray)
    } else if tagged_names_of::<nova_wallpaper_api::Command>("cmd").contains(&name) {
        serde_json::from_value(request).map(IpcCommand::Wallpaper)
    } else {
        serde_json::from_value(request).map(IpcCommand::Shared)
    };
    read.map_err(|e| format!("novad has no such request: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_checked_before_sending() {
        assert_eq!(parse("status", None), Ok(IpcCommand::Shared(Command::Status)));
        assert_eq!(
            parse("focus_workspace", Some(r#"{"id": 3}"#)),
            Ok(IpcCommand::Shared(Command::FocusWorkspace { id: 3 }))
        );
        assert_eq!(
            parse("watch_resources", Some(r#"{"watching": true}"#)),
            Ok(IpcCommand::Resources(nova_resources_api::Command::WatchResources { watching: true }))
        );
        assert_eq!(
            parse("media_show", Some(r#"{"player": "org.mpris.MediaPlayer2.mpv"}"#)),
            Ok(IpcCommand::Media(nova_media_api::Command::MediaShow { player: "org.mpris.MediaPlayer2.mpv".into() }))
        );
        assert_eq!(
            parse("set_wifi", Some(r#"{"enabled": true}"#)),
            Ok(IpcCommand::Network(nova_network_api::Command::SetWifi { enabled: true }))
        );
        assert!(parse("forget_wifi", Some(r#"{"ssid": "x", "extra": 1}"#)).is_err());
        assert_eq!(
            parse("dismiss_notification", Some(r#"{"id": 7}"#)),
            Ok(IpcCommand::Notifications(nova_notifications_api::Command::DismissNotification { id: 7 }))
        );
        assert!(parse("end_app", Some(r#"{"key": "x", "extra": 1}"#)).is_err());
        assert_eq!(
            parse("cancel_capture", None),
            Ok(IpcCommand::Screenshot(nova_screenshot_api::Command::CancelCapture))
        );
        assert_eq!(parse("lock", None), Ok(IpcCommand::Lock(nova_lock_api::Command::Lock)));
        assert_eq!(
            parse("power", Some(r#"{"action": "suspend"}"#)),
            Ok(IpcCommand::Power(nova_power_api::Command::Power { action: nova_power_api::PowerAction::Suspend }))
        );
        assert_eq!(
            parse("refresh_ai_usage", None),
            Ok(IpcCommand::AiUsage(nova_ai_usage_api::Command::RefreshAiUsage))
        );
        assert_eq!(parse("wallpapers", None), Ok(IpcCommand::Wallpaper(nova_wallpaper_api::Command::Wallpapers)));
        assert!(parse("focus_workspace", Some(r#"{"id": 3, "extra": 1}"#)).is_err());
        assert!(parse("focus_workspace", None).is_err());
        assert!(parse("make_coffee", None).unwrap_err().contains("no such request"));
        assert!(parse("status", Some("{")).unwrap_err().contains("aren't JSON"));
    }
}
