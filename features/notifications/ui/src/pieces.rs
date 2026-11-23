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

fn device_glyph(kind: PeripheralKind) -> Glyph {
    match kind {
        PeripheralKind::Mouse | PeripheralKind::Touchpad => Glyph::MOUSE,
        PeripheralKind::Keyboard => Glyph::KEYBOARD,
        PeripheralKind::GamingInput => Glyph::GAMEPAD,
        PeripheralKind::Headset | PeripheralKind::Headphones => Glyph::HEADPHONES,
        _ => Glyph::DEVICE,
    }
}

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

pub fn meter(share: f32, height: Pixels, cx: &App) -> Div {
    let theme = Theme::of(cx);
    div()
        .w_full()
        .h(height)
        .rounded_full()
        .bg(theme.track())
        .child(div().h_full().w(relative(share.clamp(0.0, 1.0))).rounded_full().bg(theme.primary))
}
