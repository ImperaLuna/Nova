//! The region picker's toolbar, where the central pill sits: what to take
//! (a region or the whole screen) and close. It grows in from the pill's
//! height as the picker opens, its glass and what is on it together.
//! `ui/capture/CaptureToolbar.qml` and `CaptureButton.qml` in today's
//! shell; its switch to a recording and the sound buttons come with the
//! recording's port.

use crate::tokens::{BAR_HEIGHT, BUTTON_HEIGHT, BUTTON_PAD};
use gpui::{
    App, Bounds, CursorStyle, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, SharedString,
    Styled, TextRun, Window, bounds, div, font, point, size,
};
use nova_look::tokens::{
    BAR_GAP, BAR_HEIGHT as PILL_HEIGHT, DIVIDER_HEIGHT, DIVIDER_WIDTH, FONT_SANS, FONT_SIZE, ICON_GAP, ICON_SIZE,
    WEIGHT_LABEL,
};
use nova_look::{Glass, Glyph, Icon, Theme};
use nova_screenshot_api::CaptureMode;

/// What a toolbar button asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Mode(CaptureMode),
    Close,
}

const MODES: [(CaptureMode, Glyph, &str); 2] =
    [(CaptureMode::Region, Glyph::REGION, "Region"), (CaptureMode::Screen, Glyph::MONITOR, "Screen")];

/// The toolbar's widths at full size: each mode button's, and all of it.
pub struct Layout {
    buttons: [Pixels; 2],
    pub width: Pixels,
}

impl Layout {
    pub fn of(window: &Window) -> Self {
        let buttons = MODES.map(|(_, _, label)| ICON_SIZE + ICON_GAP + label_width(label, window) + BUTTON_PAD * 2.0);
        // The gaps at its ends and between the buttons, the divider and
        // the gaps around it, and close.
        let width =
            BAR_GAP + buttons[0] + BAR_GAP + buttons[1] + BAR_GAP + DIVIDER_WIDTH + BAR_GAP + BUTTON_HEIGHT + BAR_GAP;
        Self { buttons, width }
    }

    /// Where the glass is on a screen `screen_width` wide, grown in to
    /// `grown` (0 at the pill's height, 1 full).
    pub fn bounds(&self, screen_width: Pixels, grown: f32) -> Bounds<Pixels> {
        let scale = scale(grown);
        let (width, height) = (self.width * scale, BAR_HEIGHT * scale);
        // Placed on whole pixels at full size, as the QML rounds its x.
        let left = ((screen_width - self.width) / 2.0).round();
        let center = point(left + self.width / 2.0, BAR_GAP + BAR_HEIGHT / 2.0);
        bounds(point(center.x - width / 2.0, center.y - height / 2.0), size(width, height))
    }
}

/// The scale the toolbar is at, grown in to `grown`.
fn scale(grown: f32) -> f32 {
    let from = f32::from(PILL_HEIGHT) / f32::from(BAR_HEIGHT);
    from + (1.0 - from) * grown
}

fn label_width(label: &str, window: &Window) -> Pixels {
    let mut face = font(FONT_SANS);
    face.weight = WEIGHT_LABEL;
    let run = TextRun {
        len: label.len(),
        font: face,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().shape_line(SharedString::from(label), FONT_SIZE, &[run], None).width
}

/// How the toolbar is drawn this frame.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    /// Where its glass is ([`Layout::bounds`]).
    pub glass: Bounds<Pixels>,
    /// How far it has grown in, and faded in.
    pub grown: f32,
    pub shown: f32,
    /// The mode picked.
    pub mode: CaptureMode,
}

/// The toolbar as `look` says; `ask` runs what a button asks.
pub fn toolbar(
    layout: &Layout,
    look: &Look,
    ask: impl Fn(Ask, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let Look { glass, grown, shown, mode } = *look;
    let theme = Theme::of(cx);
    let scale = scale(grown);
    let at = |value: Pixels| value * scale;
    let button =
        |id: &'static str, width: Pixels, selected: bool, glyph: Glyph, label: Option<&'static str>, asked: Ask| {
            let ink = if selected { theme.on_accent } else { theme.fg };
            let ask = ask.clone();
            let mut button = div()
                .id(id)
                .flex_none()
                .w(at(width))
                .h(at(BUTTON_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .gap(at(ICON_GAP))
                .rounded_full()
                .cursor(CursorStyle::PointingHand)
                .text_size(at(FONT_SIZE))
                .font_weight(WEIGHT_LABEL)
                .text_color(ink)
                .child(Icon::new(glyph).size(at(ICON_SIZE)).color(ink))
                .children(label)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    ask(asked, window, cx);
                });
            if selected {
                button = button.bg(theme.primary);
            } else {
                let hover = theme.hover();
                button = button.hover(move |style| style.bg(hover));
            }
            button
        };
    let divider = || div().flex_none().w(at(DIVIDER_WIDTH)).h(at(DIVIDER_HEIGHT)).bg(theme.outline());
    let modes = MODES.iter().zip(layout.buttons).map(|((each, glyph, label), width)| {
        let id = if *each == CaptureMode::Region { "region" } else { "screen" };
        button(id, width, *each == mode, *glyph, Some(*label), Ask::Mode(*each))
    });
    Glass::new(glass.size.height / 2.0)
        .shadow(shown)
        .build(cx)
        .absolute()
        .left(glass.origin.x)
        .top(glass.origin.y)
        .w(glass.size.width)
        .h(glass.size.height)
        .opacity(shown)
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .px(at(BAR_GAP))
                .gap(at(BAR_GAP))
                .children(modes)
                .child(divider())
                .child(button("close", BUTTON_HEIGHT, false, Glyph::CLOSE, None, Ask::Close)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    #[test]
    fn it_grows_from_the_pill_s_height() {
        assert!((scale(0.0) * f32::from(BAR_HEIGHT) - f32::from(PILL_HEIGHT)).abs() < 1e-4);
        assert_eq!(scale(1.0), 1.0);
    }

    #[test]
    fn it_grows_about_its_middle_under_the_bar_s_gap() {
        let layout = Layout { buttons: [px(100.0), px(100.0)], width: px(300.0) };
        let full = layout.bounds(px(1000.0), 1.0);
        assert_eq!(full, bounds(point(px(350.0), BAR_GAP), size(px(300.0), BAR_HEIGHT)));
        let small = layout.bounds(px(1000.0), 0.0);
        assert_eq!(small.center(), full.center());
        assert!(small.size.height < full.size.height);
    }
}
