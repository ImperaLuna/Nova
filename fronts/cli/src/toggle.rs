use crate::client::Client;
use clap::builder::{PossibleValuesParser, TypedValueParser};
use nova_bar_api::Panel;
use nova_screenshot_api::CaptureMode;
use nova_volume_osd_api::VolumeKey;
use std::path::Path;

#[derive(clap::Args)]
pub struct ToggleArgs {
    #[arg(value_parser = panel_names())]
    panel: Panel,
}

fn panel_names() -> impl TypedValueParser<Value = Panel> {
    PossibleValuesParser::new(Panel::ALL.map(Panel::name)).try_map(|name| name.parse::<Panel>())
}

pub fn run(args: &ToggleArgs, socket: &Path) -> Result<(), String> {
    Client::connect(socket)?.request(nova_bar_api::Command::Toggle { panel: args.panel })?;
    Ok(())
}

pub fn lock(socket: &Path) -> Result<(), String> {
    Client::connect(socket)?.request(nova_lock_api::Command::Lock)?;
    Ok(())
}

#[derive(clap::Args)]
pub struct VolumeArgs {
    #[command(subcommand)]
    key: VolumeArg,
}

#[derive(clap::Subcommand)]
enum VolumeArg {
    Up,
    Down,
    Mute,
}

pub fn volume(args: &VolumeArgs, socket: &Path) -> Result<(), String> {
    let key = match args.key {
        VolumeArg::Up => VolumeKey::Up,
        VolumeArg::Down => VolumeKey::Down,
        VolumeArg::Mute => VolumeKey::Mute,
    };
    Client::connect(socket)?.request(nova_volume_osd_api::Command::Volume { key })?;
    Ok(())
}

#[derive(clap::Args)]
pub struct ScreenshotArgs {
    #[command(subcommand)]
    mode: ScreenshotArg,
}

#[derive(clap::Subcommand)]
enum ScreenshotArg {
    Region,
    Screen,
}

pub fn screenshot(args: &ScreenshotArgs, socket: &Path) -> Result<(), String> {
    let mode = match args.mode {
        ScreenshotArg::Region => CaptureMode::Region,
        ScreenshotArg::Screen => CaptureMode::Screen,
    };
    Client::connect(socket)?.request(nova_screenshot_api::Command::Screenshot { mode })?;
    Ok(())
}

pub fn record(socket: &Path) -> Result<(), String> {
    Client::connect(socket)?.request(nova_recording_api::Command::Record)?;
    Ok(())
}
