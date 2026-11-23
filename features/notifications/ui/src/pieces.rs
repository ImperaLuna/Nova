//! Pieces the pop-ups, the list and the settings page share: who a
//! notification is from, how full a low battery alert's device is, and a
//! meter. The round buttons and the on/off switch are `nova_look`'s.

use crate::app_icon::AppIcon;
use crate::logic::battery_fill;
use crate::tokens::{DEVICE_GLYPH, DEVICE_RADIUS};
use gpui::{
    AnyElement, App, Div, IntoElement, ObjectFit, ParentElement, Pixels, Styled, StyledImage, div, img, relative,
};
use nova_look::tokens::{FONT_SIZE_SMALL, WEIGHT_TITLE};
use nova_look::{DeviceBattery, Glyph, Icon, Theme};
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
    div()
        .flex()
        .items_center()
        .gap(nova_look::tokens::GAP)
        .child(DeviceBattery::new(battery_fill(battery)).low(true))
        .child(div().text_size(FONT_SIZE_SMALL).font_weight(WEIGHT_TITLE).text_color(theme.error).child(words))
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
