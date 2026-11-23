//! What the volume-osd feature draws with GPUI: the volume capsule, an
//! overlay (host `shell.overlays`, view `volume-osd.capsule` in
//! `features/volume-osd/feature.toml`) with a window of its own on each
//! screen. `ui/volume` in today's shell, which stays the spec until it is
//! removed.
//!
//! It follows novad's `sound` topic for the output in use and the `volume`
//! topic for the volume keys (which novad itself steps for this UI), and
//! changes the volume with the sound feature's commands.
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `capsule`: when it comes out, where, and for how long; `window`: one
//!   screen's window, which draws it and takes the pointer.
//! - `motion`, `logic`: the springs and what `volume.js` worked out,
//!   tested alone; `shader` and `liquid.wgsl`: its glass and liquid.

mod capsule;
mod logic;
mod motion;
mod shader;
mod tokens;
mod window;

use gpui::App;

/// Registers the capsule with the stage.
pub fn register(cx: &mut App) {
    nova_stage::overlay("volume-osd.capsule", cx, |cx| Box::new(capsule::Capsule::start(cx)));
}
