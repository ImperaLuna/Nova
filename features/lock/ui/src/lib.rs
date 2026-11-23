//! What the lock feature draws with GPUI (AGENTS.md, "Porting a feature's UI to
//! GPUI"): the lock screen (`lock.screen`, host `shell.overlays`).
//!
//! novad says when to lock (`nova lock`, the power menu, before sleep);
//! the compositor's session lock (ext-session-lock-v1) then shows nothing
//! but this program's lock surfaces, one on every screen, until the user's
//! password checks out here. Only that unlocks: no message from novad does,
//! and closing lock surfaces doesn't (a screen unplugged, even the last).
//! If this program dies while locked, the compositor keeps the screens
//! covered, and novad tells the restarted one to lock again.
//!
//! - [`lock`]: the lock itself: novad, the compositor, the windows and the
//!   check.
//! - `surface`: one screen's window and what it draws.
//! - `prompt`: what the prompt works out without drawing, tested alone.
//! - `pam`: the password check.
//! - `tokens`: the lock screen's own sizes and timings.

mod lock;
mod pam;
mod prompt;
mod surface;
mod tokens;

use gpui::App;
use lock::LockScreen;

/// Registers the lock feature's view with the stage.
pub fn register(cx: &mut App) {
    nova_stage::overlay("lock.screen", cx, |cx| Box::new(LockScreen::get(cx)));
}
