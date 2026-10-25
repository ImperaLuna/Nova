//! `nova toggle <panel>`, `nova lock`, `nova volume <key>`,
//! `nova screenshot <mode>` and `nova record`: ask novad to open or close a
//! panel on the focused output, to lock the screen, to pass a volume key to
//! the UI, to take a screenshot, or to start or stop recording, for
//! compositor keybinds. They print nothing
//! when it worked, so a keybind stays quiet; novad's reason goes to stderr
//! when it didn't.

use crate::client::Client;
use clap::builder::{PossibleValuesParser, TypedValueParser};
use nova_protocol::{CaptureMode, Command, Panel, VolumeKey};
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

#[derive(clap::Args)]
pub struct VolumeArgs {
    #[command(subcommand)]
    key: VolumeArg,
}

#[derive(clap::Subcommand)]
enum VolumeArg {
    /// One step louder, up to 100%.
    Up,
    /// One step quieter.
    Down,
    /// Mute, or unmute when muted.
    Mute,
}

pub fn volume(args: &VolumeArgs, socket: &Path) -> Result<(), String> {
    let key = match args.key {
        VolumeArg::Up => VolumeKey::Up,
        VolumeArg::Down => VolumeKey::Down,
        VolumeArg::Mute => VolumeKey::Mute,
    };
    Client::connect(socket)?.request(Command::Volume { key })?;
    Ok(())
}

#[derive(clap::Args)]
pub struct ScreenshotArgs {
    #[command(subcommand)]
    mode: ScreenshotArg,
}

#[derive(clap::Subcommand)]
enum ScreenshotArg {
    /// Freeze the screens and drag out a region; again closes the picker.
    Region,
    /// The whole focused screen, at once.
    Screen,
}

pub fn screenshot(args: &ScreenshotArgs, socket: &Path) -> Result<(), String> {
    let mode = match args.mode {
        ScreenshotArg::Region => CaptureMode::Region,
        ScreenshotArg::Screen => CaptureMode::Screen,
    };
    Client::connect(socket)?.request(Command::Screenshot { mode })?;
    Ok(())
}

pub fn record(socket: &Path) -> Result<(), String> {
    Client::connect(socket)?.request(Command::Record)?;
    Ok(())
}
