//! `nova-ui`: the program that draws Nova's UI with GPUI. novad runs it in
//! place of Quickshell when `features.toml` says `ui = "gpui"`, hands it
//! the socket (`NOVA_SOCKET`) and restarts it when it exits
//! (`app/src/supervisor.rs`).
//!
//! It starts GPUI with the look kit's icons as its assets, connects to
//! novad, lets every ported feature's ui register its views
//! ([`register_features`]) and starts the stage, which shows them on every
//! screen. Wiring only: it also hands the stage what novad sends that is no
//! feature's ui to follow, the theme's colors and `nova toggle`.
//!
//! By hand, `--demo` puts test pills and panels on the stage ([`demo`]) and
//! `--replay` runs against recordings where the real novad can't
//! ([`replay`]).

mod args;
mod demo;
mod replay;

use args::Args;
use gpui::{App, QuitMode, SharedString, Subscription};
use gpui_platform::application;
use nova_daemon_info::SOCKET_VAR;
use nova_desktop_info::{Capability, Desktop, Output};
use nova_look::{IconAssets, Theme};
use nova_stage::{Stage, StageOptions};
use nova_ui_client::Daemon;
use nova_ui_client::fake::FakeDaemon;
use std::path::PathBuf;
use std::process::ExitCode;

/// GPUI's errors, as nova-ui's own lines; its warnings and below are
/// routine.
struct GpuiErrors;

impl log::Log for GpuiErrors {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Error
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("ERROR nova-ui: gpui: {}", record.args());
        }
    }

    fn flush(&self) {}
}

fn main() -> ExitCode {
    // Only when none is set yet, which nothing else in nova-ui does.
    if log::set_logger(&GpuiErrors).is_ok() {
        log::set_max_level(log::LevelFilter::Error);
    }
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(problem) => {
            eprintln!("nova-ui: {problem}\n\n{}", args::USAGE);
            return ExitCode::from(2);
        }
    };
    // novad logs each line the UI prints, and reads the level from a line
    // that opens with one (`nova_log`'s `ui_line`).
    eprintln!("INFO nova-ui {} started", env!("CARGO_PKG_VERSION"));
    let (socket, _replayed) = match socket(&args) {
        Ok(found) => found,
        Err(problem) => {
            eprintln!("ERROR nova-ui: {problem}");
            return ExitCode::FAILURE;
        }
    };
    // Only `--demo-toggles` quits on purpose: anything else that ends GPUI's
    // event loop (a Wayland protocol error, the compositor gone) is a
    // crash, which novad's supervisor logs and restarts with a backoff.
    let quits_by_itself = args.demo_toggles.is_some();
    application().with_assets(IconAssets).run(move |cx: &mut App| {
        // GPUI quits a Linux program when its last window closes, which
        // every window does when the last screen goes (a monitor turned
        // off). The lock screen must outlive that: the session lock is
        // this process's, and a compositor that sees its locker die may
        // not let the next one in.
        cx.set_quit_mode(QuitMode::Explicit);
        Daemon::connect(socket, cx);
        // Followed for as long as the program runs.
        for followed in follow_novad(cx) {
            followed.detach();
        }
        register_features(cx);
        let extra_slots = if args.demo { demo::slots(cx) } else { Vec::new() };
        Stage::start(StageOptions { extra_slots, log_frames: args.frames }, cx);
        if let Some(times) = args.demo_toggles {
            let panel = args.demo_panel.unwrap_or_else(|| demo::CENTER_PANEL.into());
            demo::toggle_by_itself(panel, times, cx);
        }
    });
    if quits_by_itself {
        return ExitCode::SUCCESS;
    }
    eprintln!("ERROR nova-ui: GPUI's event loop stopped, most likely because the compositor closed the connection");
    ExitCode::FAILURE
}

/// Each ported feature's ui registers its views with the stage here, one
/// line per feature, sorted by feature id:
/// `nova_<id>_ui::register(cx);`
fn register_features(cx: &mut App) {
    nova_ai_usage_ui::register(cx);
    nova_bar_ui::register(cx);
    nova_bluetooth_ui::register(cx);
    nova_clock_ui::register(cx);
    nova_keep_awake_ui::register(cx);
    nova_launcher_ui::register(cx);
    nova_lock_ui::register(cx);
    nova_media_ui::register(cx);
    nova_microphone_ui::register(cx);
    nova_network_ui::register(cx);
    nova_notifications_ui::register(cx);
    nova_peripherals_ui::register(cx);
    nova_power_ui::register(cx);
    nova_quick_settings_ui::register(cx);
    nova_resources_ui::register(cx);
    nova_screenshot_ui::register(cx);
    nova_sound_ui::register(cx);
    nova_tray_ui::register(cx);
    nova_volume_osd_ui::register(cx);
    nova_wallpaper_ui::register(cx);
    nova_workspaces_ui::register(cx);
}

/// What novad sends that no feature's ui follows: the theme's colors, which
/// repaint every window; `nova toggle`, which opens a panel on the focused
/// output (the stage names no feature, so the bar's `Panel` list meets it
/// here); and which outputs show a fullscreen window, where the bar steps
/// aside.
fn follow_novad(cx: &mut App) -> [Subscription; 3] {
    let theme = Daemon::on_event(nova_theme_info::TOPIC, cx, |nova_theme_info::Event::Theme(theme), cx| {
        Theme::set(Theme::from(&theme), cx);
    });
    let toggle = Daemon::on_event(nova_bar_api::TOPIC, cx, |nova_bar_api::Event::Toggle { panel, output }, cx| {
        Stage::toggle_on_output(panel.name(), output.as_deref(), cx);
    });
    let fullscreen =
        Daemon::on_event(nova_desktop_info::TOPIC, cx, |nova_desktop_info::Event::Compositor(desktop), cx| {
            Stage::set_fullscreen(fullscreen_outputs(&desktop), cx);
        });
    [theme, toggle, fullscreen]
}

/// The outputs whose shown workspace has a window over the whole screen,
/// when the compositor says (`Protocol.fullscreenShown`).
fn fullscreen_outputs(desktop: &Desktop) -> Vec<SharedString> {
    if !desktop.capabilities.contains(&Capability::Fullscreen) {
        return Vec::new();
    }
    let shows_fullscreen = |output: &&Output| output.workspaces.iter().any(|each| each.active && each.fullscreen);
    desktop.outputs.iter().filter(shows_fullscreen).map(|output| output.name.clone().into()).collect()
}

/// Where novad listens: the fake one's socket with `--replay` (it runs
/// until the value beside the path is dropped), else the one novad handed
/// over.
fn socket(args: &Args) -> Result<(PathBuf, Option<FakeDaemon>), String> {
    if let Some((socket, novad)) = replay::start(&args.replay)? {
        return Ok((socket, Some(novad)));
    }
    match std::env::var_os(SOCKET_VAR) {
        Some(socket) => Ok((socket.into(), None)),
        None => Err(format!(
            "{SOCKET_VAR} isn't set. novad sets it when it starts nova-ui; by hand, set it to novad's socket \
             or pass --replay."
        )),
    }
}
