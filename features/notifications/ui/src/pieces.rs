//! Pieces the pop-ups, the list and the settings page share: who a
//! notification is from, a low battery alert's battery, the small round
//! buttons and the on/off switch.

use crate::app_icon::AppIcon;
use crate::logic::battery_fill;
use crate::tokens::{
    BATTERY_HEIGHT, BATTERY_INSET, BATTERY_NUB_HEIGHT, BATTERY_NUB_WIDTH, BATTERY_RADIUS, BATTERY_RIM, BATTERY_WIDTH,
    DEVICE_GLYPH, DEVICE_RADIUS, SWITCH_HEIGHT, SWITCH_KNOB, SWITCH_WIDTH,
};
use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, InteractiveElement, IntoElement, ObjectFit, ParentElement, Pixels,
    StatefulInteractiveElement, Styled, StyledImage, Window, div, img, relative,
};
use nova_look::tokens::{FONT_SIZE_SMALL, ICON_SIZE_SMALL, WEIGHT_TITLE};
use nova_look::{Glyph, Icon, Theme};
use nova_notifications_api::Notification;
use nova_peripherals_api::{Battery, PeripheralKind};
use std::path::PathBuf;

/// The glyph of a device's kind.
fn device_glyph(kind: PeripheralKind) -> Glyph {
    match kind {
        PeripheralKind::Mouse | PeripheralKind::Touchpad => Glyph::MOUSE,
        PeripheralKind::Keyboard => Glyph::KEYBOARD,
        PeripheralKind::GamingInput => Glyph::GAMEPAD,
        PeripheralKind::Headset | PeripheralKind::Headphones => Glyph::HEADPHONES,
        _ => Glyph::DEVICE,
    }
}

/// Who a notification is from, `size` square: for Nova's low battery
/// alerts, the device's glyph; else the picture it came with (a sender's
/// photo), round; or else the app's icon, which falls back to its letter.
pub fn who(notification: &Notification, size: Pixels, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    if let Some(device) = notification.device {
        return div()
            .size(size)
            .flex()
            .items_center()
            .justify_center()
            .rounded(size * DEVICE_RADIUS)
            .bg(theme.surface_raised())
            .child(Icon::new(device_glyph(device.kind)).size((size * DEVICE_GLYPH).round()))
            .into_any_element();
    }
    let app_icon = {
        let id = notification.app_id.clone().unwrap_or_else(|| notification.app.clone());
        let (name, icon) = (notification.app.clone(), notification.icon.clone());
        move || AppIcon::new(id.clone(), name.clone(), size).icon(icon.as_deref()).into_any_element()
    };
    match notification.image.as_deref().filter(|path| path.starts_with('/')) {
        Some(picture) => img(PathBuf::from(picture))
            .size(size)
            .rounded_full()
            .object_fit(ObjectFit::Cover)
            .with_fallback(app_icon.clone())
            .with_loading(app_icon)
            .into_any_element(),
        None => app_icon(),
    }
}

/// How full a low battery alert's device is, at the end of its title line:
/// its battery and its percent, or the level novad words it as (its body)
/// when it gives none.
pub fn battery_amount(notification: &Notification, battery: Battery, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let words = match battery {
        Battery::Percent(percent) => format!("{percent}%"),
        Battery::Level(_) => notification.body.clone(),
    };
    let level = div()
        .absolute()
        .top(BATTERY_INSET)
        .left(BATTERY_INSET)
        .bottom(BATTERY_INSET)
        .w((BATTERY_WIDTH - BATTERY_INSET * 2.0) * battery_fill(battery))
        .rounded(BATTERY_RADIUS - BATTERY_INSET / 2.0)
        .bg(theme.error);
    let body = div()
        .relative()
        .w(BATTERY_WIDTH)
        .h(BATTERY_HEIGHT)
        .rounded(BATTERY_RADIUS)
        .border(BATTERY_RIM)
        .border_color(theme.error)
        .child(level);
    let nub = div().w(BATTERY_NUB_WIDTH).h(BATTERY_NUB_HEIGHT).rounded_full().bg(theme.error);
    div()
        .flex()
        .items_center()
        .gap(nova_look::tokens::GAP)
        .child(div().flex().items_center().child(body).child(nub))
        .child(div().text_size(FONT_SIZE_SMALL).font_weight(WEIGHT_TITLE).text_color(theme.error).child(words))
}

/// A small round icon button, raised: the list's ✕, ⋯, bell and gear.
/// `on` fills it with the accent (do not disturb); `quiet` leaves it clear
/// until hovered, for buttons that sit on a card.
pub struct RoundButton {
    id: ElementId,
    glyph: Glyph,
    filled: bool,
    size: Pixels,
    on: bool,
    quiet: bool,
}

impl RoundButton {
    pub fn new(id: impl Into<ElementId>, glyph: Glyph, size: Pixels) -> Self {
        Self { id: id.into(), glyph, filled: false, size, on: false, quiet: false }
    }

    /// Its glyph drawn solid too (the ⋯).
    pub fn filled(mut self) -> Self {
        self.filled = true;
        self
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    pub fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }

    pub fn build(
        self,
        cx: &App,
        clicked: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> gpui::Stateful<Div> {
        let theme = Theme::of(cx);
        let mut icon = Icon::new(self.glyph).size(ICON_SIZE_SMALL);
        if self.filled {
            icon = icon.filled(self.glyph);
        }
        let (fill, hover) = match (self.on, self.quiet) {
            (true, _) => (Some(theme.primary), theme.primary),
            (false, true) => (None, theme.hover()),
            (false, false) => (Some(theme.surface_raised()), theme.hover()),
        };
        let button = div()
            .id(self.id)
            .size(self.size)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .child(icon.color(if self.on { theme.on_accent } else { theme.fg }))
            .on_click(move |event, window, cx| {
                cx.stop_propagation();
                clicked(event, window, cx);
            });
        match fill {
            Some(fill) => button.bg(fill),
            None => button,
        }
    }
}

/// An on/off switch: the accent while on, a quiet track while off. It only
/// shows; the row it sits in takes the click.
pub fn switch(on: bool, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let inset = (SWITCH_HEIGHT - SWITCH_KNOB) / 2.0;
    let knob = div().absolute().top(inset).size(SWITCH_KNOB).rounded_full();
    let knob = if on { knob.right(inset).bg(theme.on_accent) } else { knob.left(inset).bg(theme.fg_soft()) };
    div()
        .relative()
        .flex_none()
        .w(SWITCH_WIDTH)
        .h(SWITCH_HEIGHT)
        .rounded_full()
        .bg(if on { theme.primary } else { theme.track() })
        .child(knob)
}

/// A bar `share` (0 to 1) of the way along a track.
pub fn meter(share: f32, height: Pixels, cx: &App) -> Div {
    let theme = Theme::of(cx);
    div()
        .w_full()
        .h(height)
        .rounded_full()
        .bg(theme.track())
        .child(div().h_full().w(relative(share.clamp(0.0, 1.0))).rounded_full().bg(theme.primary))
}
