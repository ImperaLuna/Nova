use gpui::{
    Bounds, DisplayId, Pixels, Size, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
    layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions},
    point, px,
};

pub const NAMESPACE_PREFIX: &str = "nova-";
pub const NAMESPACE: &str = "nova-shell";
pub const APP_ID: &str = "nova";
// Without -1 Hyprland moves the stage down by the bar's reserved space:
// https://wayland.app/protocols/wlr-layer-shell-unstable-v1#zwlr_layer_surface_v1:request:set_exclusive_zone
pub const IGNORE_RESERVED_SPACE: Pixels = px(-1.0);

#[derive(Debug, Clone, PartialEq)]
pub struct ShellWindow {
    pub display: DisplayId,
    pub size: Size<Pixels>,
    pub layer: Layer,
    pub anchor: Anchor,
    pub keyboard: KeyboardInteractivity,
    pub exclusive_zone: Pixels,
    pub namespace: &'static str,
}

impl ShellWindow {
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
            // GPUI caps a window without focus at 30 frames a second, and a shell surface never has an app's focus:
            // https://github.com/ImperaLuna/zed/tree/2033650502b1d0ec524b8913bf5a01b9463bed5d/crates/gpui
            inactive_frame_interval: None,
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

pub fn lock_surface(display: DisplayId, size: Size<Pixels>) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin: point(px(0.0), px(0.0)), size })),
        display_id: Some(display),
        titlebar: None,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        app_id: Some(APP_ID.into()),
        inactive_frame_interval: None,
        window_background: WindowBackgroundAppearance::Opaque,
        kind: WindowKind::SessionLock,
        ..Default::default()
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

    #[test]
    fn a_lock_surface_hides_what_is_behind_and_draws_at_the_screen_s_rate() {
        let options = lock_surface(DisplayId::new(1), size(px(1920.0), px(1080.0)));
        assert_eq!(options.kind, WindowKind::SessionLock);
        assert_eq!(options.window_background, WindowBackgroundAppearance::Opaque);
        assert_eq!(options.inactive_frame_interval, None);
    }
}
