//! `nova-ui`: the program that draws Nova's UI with GPUI. novad runs it in
//! place of Quickshell when `features.toml` says `ui = "gpui"`, hands it
//! the socket (`NOVA_SOCKET`) and restarts it when it exits
//! (`app/src/supervisor.rs`).
//!
//! It opens nothing yet: it starts GPUI and waits until novad stops it. The
//! socket client, the look kit, the stage and each feature's ui crate plug
//! in here as the GPUI ports land (IMP-368).

use gpui::App;
use gpui_platform::application;

fn main() {
    // novad logs each line the UI prints, and reads the level from a line
    // that opens with one (`nova_log`'s `ui_line`).
    eprintln!("INFO nova-ui {} started", env!("CARGO_PKG_VERSION"));
    application().run(|_: &mut App| {});
}
