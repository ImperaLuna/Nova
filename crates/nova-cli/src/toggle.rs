//! `nova toggle <panel>` and `nova lock`: ask novad to open or close a panel
//! on the focused output, or to lock the screen, for compositor keybinds.
//! They print nothing when it worked, so a keybind stays quiet; novad's
//! reason goes to stderr when it didn't.

use crate::client::Client;
use clap::builder::{PossibleValuesParser, TypedValueParser};
use nova_protocol::{Command, Panel};
use std::path::Path;

#[derive(clap::Args)]
pub struct ToggleArgs {
    /// The panel to open or close.
    #[arg(value_parser = panel_names())]
    panel: Panel,
}

/// Accepts exactly the panels nova-protocol knows, and lists them in help.
fn panel_names() -> impl TypedValueParser<Value = Panel> {
    PossibleValuesParser::new(Panel::ALL.map(Panel::name))
        .map(|name| name.parse().expect("PossibleValuesParser only passes listed names"))
}

pub fn run(args: &ToggleArgs, socket: &Path) -> Result<(), String> {
    Client::connect(socket)?.request(Command::Toggle { panel: args.panel })?;
    Ok(())
}

pub fn lock(socket: &Path) -> Result<(), String> {
    Client::connect(socket)?.request(Command::Lock)?;
    Ok(())
}
