//! The one place a shell window's options are built.
//!
//! Everything Nova puts on screen is a layer surface: see-through, without
//! a title bar, and never focused the way an app's window is. GPUI takes a
//! window without focus for one in the background and draws it at 30
//! frames a second at most, which would halve every animation in the
//! shell. [`ShellWindow::options`] turns that cap off, along with the rest
//! every shell window shares, so no feature builds `WindowOptions` itself
//! (`scripts/lint-traps.sh` checks that).

use gpui::{
    Bounds, DisplayId, Pixels, Size, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
    layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions},
    point, px,
};

/// What every Nova surface's namespace starts with. Compositor rules match
/// on it (`packaging/compositor/niri.kdl` turns on blur of the windows
/// behind for `^nova-`), so a surface that names itself keeps the prefix.
pub const NAMESPACE_PREFIX: &str = "nova-";
/// The namespace of a surface that did not name itself.
pub const NAMESPACE: &str = "nova-shell";
/// The app id of Nova's windows.
pub const APP_ID: &str = "nova";
/// An exclusive zone that keeps a surface where it is anchored whatever
/// room other surfaces reserved: without it Hyprland moves the stage down
/// by the bar's reserved space.
pub const IGNORE_RESERVED_SPACE: Pixels = px(-1.0);

/// A window of the shell: a layer surface on one screen.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellWindow {
    /// The screen it is on.
    pub display: DisplayId,
    pub size: Size<Pixels>,
    pub layer: Layer,
    /// The screen edges it is fixed to.
    pub anchor: Anchor,
    /// Whether it takes the keyboard: none for most, while a panel with a
    /// field is open for the stage.
    pub keyboard: KeyboardInteractivity,
    /// The room it reserves from app windows along its edge, or
    /// [`IGNORE_RESERVED_SPACE`].
    pub exclusive_zone: Pixels,
    /// What compositor rules name it by.
    pub namespace: &'static str,
}

impl ShellWindow {
    /// A window across the top of `display`, above app windows, that takes
    /// no keyboard and reserves no room.
    pub fn new(display: DisplayId, size: Size<Pixels>) -> Self {
        Self {
            display,
            size,
            layer: Layer::Top,
            anchor: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
            keyboard: KeyboardInteractivity::None,
            exclusive_zone: IGNORE_RESERVED_SPACE,
            namespace: NAMESPACE,
        }
    }

    /// The options to open it with.
    pub fn options(self) -> WindowOptions {
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds { origin: point(px(0.0), px(0.0)), size: self.size })),
            display_id: Some(self.display),
            titlebar: None,
            focus: false,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            app_id: Some(APP_ID.into()),
            // A shell surface never has the focus an app's window has, so
            // GPUI would take it for a background window and cap it at 30
            // frames a second.
            inactive_frame_interval: None,
            // Glass needs the compositor's blur behind it. The window's
            // owner limits it to the glass (`crate::set_blur_region`).
            window_background: WindowBackgroundAppearance::Blurred,
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: self.namespace.into(),
                layer: self.layer,
                anchor: self.anchor,
                exclusive_zone: Some(self.exclusive_zone),
                keyboard_interactivity: self.keyboard,
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    #[test]
    fn every_shell_window_draws_at_the_screen_s_rate() {
        let options = ShellWindow::new(DisplayId::new(1), size(px(1920.0), px(60.0))).options();
        assert_eq!(options.inactive_frame_interval, None);
        assert_eq!(options.window_background, WindowBackgroundAppearance::Blurred);
        assert!(options.titlebar.is_none() && !options.focus);
        let WindowKind::LayerShell(layer) = options.kind else {
            panic!("a shell window is a layer surface, not {:?}", options.kind);
        };
        assert_eq!(layer.namespace, NAMESPACE);
        assert!(NAMESPACE.starts_with(NAMESPACE_PREFIX));
        assert_eq!(layer.exclusive_zone, Some(IGNORE_RESERVED_SPACE));
        assert_eq!(layer.keyboard_interactivity, KeyboardInteractivity::None);
    }
}
