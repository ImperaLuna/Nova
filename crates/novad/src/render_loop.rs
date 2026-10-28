//! Qt's render loop for the UI: the basic one, which draws every window on
//! one thread, unless the session picks one itself.
//!
//! The threaded loop gives each window a render thread of its own, which
//! cost NovaUI about 30 MB Pss on NVIDIA (IMP-159), and it can stop drawing
//! a surface for good (IMP-207, IMP-208). Qt's Wayland plugin waits at most
//! 100 ms for a frame callback before a swap (whatever
//! QT_WAYLAND_FRAME_CALLBACK_TIMEOUT says) and marks the window unexposed
//! when it's later; the threaded loop then takes the window off its render
//! thread, and gives it back only on the expose that comes with the late
//! callback. A surface that has stopped drawing by then (Quickshell
//! `updatesEnabled` off, as Nova's surfaces are while nothing on them
//! changes) has that expose dropped, and never draws again: a panel opened
//! on it never got the keyboard, and the click catcher never left the
//! Overlay layer. The basic loop checks whether a window is exposed each
//! time it draws one, so a late callback only delays a frame.

use std::ffi::OsString;

pub const VAR: &str = "QSG_RENDER_LOOP";
const BASIC: &str = "basic";

/// The render loop to start the UI with: the basic one, unless the
/// session (`env`) picks one itself.
pub fn for_ui(env: impl Fn(&str) -> Option<OsString>) -> Option<&'static str> {
    env(VAR).is_none().then_some(BASIC)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ui_gets_the_basic_loop_unless_the_session_picks() {
        assert_eq!(for_ui(|_| None), Some(BASIC));
        let set = |name: &str| (name == VAR).then(|| OsString::from("threaded"));
        assert_eq!(for_ui(set), None);
    }
}
