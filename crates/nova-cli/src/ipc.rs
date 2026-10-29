//! `nova ipc`: one raw request to novad, its result printed as JSON, for
//! scripts. The command and its arguments are checked against
//! nova-protocol before anything is sent. `subscribe` instead prints the
//! events of its topics, one JSON object per line, until novad stops or the
//! command is interrupted (watching a topic from a shell, like
//! `scripts/desktop/isolated-novad` does).

use crate::client::Client;
use nova_protocol::Command;
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
    let follows = matches!(command, Command::Subscribe { .. });
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
            break;
        }
    }
    Ok(())
}

/// The command `name` with `args`, as novad would read it off the socket.
fn parse(name: &str, args: Option<&str>) -> Result<Command, String> {
    let mut request = serde_json::json!({ "cmd": name });
    if let Some(args) = args {
        request["args"] = serde_json::from_str(args).map_err(|e| format!("the arguments aren't JSON: {e}"))?;
    }
    serde_json::from_value(request).map_err(|e| format!("novad has no such request: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_checked_before_sending() {
        assert_eq!(parse("status", None), Ok(Command::Status));
        assert_eq!(parse("focus_workspace", Some(r#"{"id": 3}"#)), Ok(Command::FocusWorkspace { id: 3 }));
        assert!(parse("focus_workspace", Some(r#"{"id": 3, "extra": 1}"#)).is_err());
        assert!(parse("focus_workspace", None).is_err());
        assert!(parse("make_coffee", None).unwrap_err().contains("no such request"));
        assert!(parse("status", Some("{")).unwrap_err().contains("aren't JSON"));
    }
}
