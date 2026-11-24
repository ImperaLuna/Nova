use crate::tokens::{BAR_HEIGHT, BUTTON_HEIGHT, BUTTON_PAD, SWITCH_PAD, SWITCH_WIDTH};
use gpui::{
    App, Bounds, CursorStyle, Div, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, SharedString,
    Stateful, Styled, TextRun, Window, bounds, div, font, point, size,
};
use nova_look::tokens::{
    BAR_GAP, BAR_HEIGHT as PILL_HEIGHT, DIVIDER_HEIGHT, DIVIDER_WIDTH, FONT_SANS, FONT_SIZE, ICON_GAP, ICON_SIZE,
    WEIGHT_LABEL,
};
use nova_look::{Glass, Glyph, Icon, Theme, mix};
use nova_screenshot_api::CaptureMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Mode(CaptureMode),
    Video(bool),
    DesktopAudio,
    MicAudio,
    Close,
}

const MODES: [(CaptureMode, Glyph, &str); 2] =
    [(CaptureMode::Region, Glyph::REGION, "Region"), (CaptureMode::Screen, Glyph::MONITOR, "Screen")];
const SOUNDS: [(Ask, Glyph, &str); 2] =
    [(Ask::DesktopAudio, Glyph::SPEAKER, "Desktop"), (Ask::MicAudio, Glyph::MIC, "Mic")];

pub struct Layout {
    switch: bool,
    modes: [Pixels; 2],
    sounds: [Pixels; 2],
    base: Pixels,
    extras: Pixels,
    full: Pixels,
}

impl Layout {
    pub fn of(switch: bool, video: bool, window: &Window) -> Self {
        let wide = |label: &str| ICON_SIZE + ICON_GAP + label_width(label, window) + BUTTON_PAD * 2.0;
        let modes = MODES.map(|(_, _, label)| wide(label));
        let sounds = SOUNDS.map(|(_, _, label)| wide(label));
        let switched = if switch { SWITCH_WIDTH + BAR_GAP + DIVIDER_WIDTH + BAR_GAP } else { Pixels::ZERO };
        let base = BAR_GAP
            + switched
            + modes[0]
            + BAR_GAP
            + modes[1]
            + BAR_GAP
            + DIVIDER_WIDTH
            + BAR_GAP
            + BUTTON_HEIGHT
            + BAR_GAP;
        let extras = BAR_GAP + DIVIDER_WIDTH + BAR_GAP + sounds[0] + BAR_GAP + sounds[1];
        let full = if switch || video { base + extras } else { base };
        Self { switch, modes, sounds, base, extras, full }
    }

    pub fn bounds(&self, screen_width: Pixels, grown: f32, sounds: f32) -> Bounds<Pixels> {
        let scale = scale(grown);
        let now = self.base + self.extras * sounds;
        let (width, height) = (now * scale, BAR_HEIGHT * scale);
        let left = ((screen_width - self.full) / 2.0).round();
        let center = point(left + now / 2.0, BAR_GAP + BAR_HEIGHT / 2.0);
        bounds(point(center.x - width / 2.0, center.y - height / 2.0), size(width, height))
    }
}

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

#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub glass: Bounds<Pixels>,
    pub grown: f32,
    pub shown: f32,
    pub mode: CaptureMode,
    pub video: bool,
    pub knob: f32,
    pub sounds: f32,
    pub desktop_audio: bool,
    pub mic_audio: bool,
    pub busy: bool,
}

pub fn toolbar(
    layout: &Layout,
    look: &Look,
    ask: impl Fn(Ask, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let Look { glass, grown, shown, mode, .. } = *look;
    let theme = Theme::of(cx);
    let at = |value: Pixels| value * scale(grown);
    let button = |id: &'static str, width: Pixels, selected: bool, glyph: Glyph, label: Option<&'static str>, asked| {
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
    let gap = || div().flex_none().w(at(BAR_GAP));
    let divider = || div().flex_none().w(at(DIVIDER_WIDTH)).h(at(DIVIDER_HEIGHT)).bg(theme.outline());
    let mut row = div().absolute().inset_0().flex().items_center().child(gap());
    if layout.switch {
        row = row.child(switch(look, at, ask.clone(), cx)).child(gap()).child(divider()).child(gap());
    }
    for (index, ((each, glyph, label), width)) in MODES.iter().zip(layout.modes).enumerate() {
        let id = if *each == CaptureMode::Region { "region" } else { "screen" };
        if index > 0 {
            row = row.child(gap());
        }
        row = row.child(button(id, width, *each == mode, *glyph, Some(*label), Ask::Mode(*each)));
    }
    let picked = [look.desktop_audio, look.mic_audio];
    let mut sounds = div().flex_none().w(at(layout.extras)).flex().items_center().child(gap()).child(divider());
    for (((asked, glyph, label), width), picked) in SOUNDS.iter().zip(layout.sounds).zip(picked) {
        let id = if *asked == Ask::DesktopAudio { "desktop" } else { "mic" };
        sounds = sounds.child(gap()).child(button(id, width, picked, *glyph, Some(*label), *asked));
    }
    row = row
        .child(
            div().flex_none().w(at(layout.extras * look.sounds)).overflow_hidden().opacity(look.sounds).child(sounds),
        )
        .child(gap())
        .child(divider())
        .child(gap())
        .child(button("close", BUTTON_HEIGHT, false, Glyph::CLOSE, None, Ask::Close));
    Glass::new(glass.size.height / 2.0)
        .shadow(shown)
        .build(cx)
        .absolute()
        .left(glass.origin.x)
        .top(glass.origin.y)
        .w(glass.size.width)
        .h(glass.size.height)
        .opacity(shown)
        .overflow_hidden()
        .child(row)
}

fn switch(
    look: &Look,
    at: impl Fn(Pixels) -> Pixels,
    ask: impl Fn(Ask, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Stateful<Div> {
    let theme = Theme::of(cx);
    let knob_width = (SWITCH_WIDTH - SWITCH_PAD * 2.0) / 2.0;
    let (video, busy, knob) = (look.video, look.busy, look.knob);
    let hover = theme.hover();
    let side = |on_video: bool| {
        let lit = if on_video { knob } else { 1.0 - knob };
        let ink = mix(theme.fg_soft(), theme.on_accent, lit);
        let left = SWITCH_PAD + if on_video { knob_width } else { Pixels::ZERO };
        div()
            .absolute()
            .left(at(left))
            .top_0()
            .w(at(knob_width))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .child(Icon::new(if on_video { Glyph::VIDEO } else { Glyph::CAMERA }).size(at(ICON_SIZE)).color(ink))
    };
    div()
        .id("switch")
        .flex_none()
        .relative()
        .w(at(SWITCH_WIDTH))
        .h(at(BUTTON_HEIGHT))
        .rounded_full()
        .bg(theme.surface_raised())
        .hover(move |style| style.bg(hover))
        .cursor(CursorStyle::PointingHand)
        .child(div().absolute().inset_0().rounded_full().border(at(DIVIDER_WIDTH)).border_color(theme.outline()))
        .child(
            div()
                .absolute()
                .left(at(SWITCH_PAD + knob_width * knob))
                .top(at(SWITCH_PAD))
                .w(at(knob_width))
                .h(at(BUTTON_HEIGHT - SWITCH_PAD * 2.0))
                .rounded_full()
                .bg(theme.primary),
        )
        .child(side(false))
        .child(side(true))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            if !busy {
                ask(Ask::Video(!video), window, cx);
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    fn layout(switch: bool, video: bool) -> Layout {
        let base = px(300.0);
        let extras = px(150.0);
        let full = if switch || video { base + extras } else { base };
        Layout { switch, modes: [px(100.0); 2], sounds: [px(60.0); 2], base, extras, full }
    }

    #[test]
    fn it_grows_from_the_pill_s_height() {
        assert!((scale(0.0) * f32::from(BAR_HEIGHT) - f32::from(PILL_HEIGHT)).abs() < 1e-4);
        assert_eq!(scale(1.0), 1.0);
    }

    #[test]
    fn it_grows_about_its_middle_under_the_bar_s_gap() {
        let layout = layout(false, false);
        let full = layout.bounds(px(1000.0), 1.0, 0.0);
        assert_eq!(full, bounds(point(px(350.0), BAR_GAP), size(px(300.0), BAR_HEIGHT)));
        let small = layout.bounds(px(1000.0), 0.0, 0.0);
        assert_eq!(small.center(), full.center());
        assert!(small.size.height < full.size.height);
    }

    #[test]
    fn the_sound_buttons_slide_in_on_the_right_and_the_rest_stays_put() {
        let layout = layout(true, false);
        let out = layout.bounds(px(1000.0), 1.0, 0.0);
        let half = layout.bounds(px(1000.0), 1.0, 0.5);
        let ins = layout.bounds(px(1000.0), 1.0, 1.0);
        assert_eq!(out.origin.x, px(275.0));
        assert_eq!(half.origin.x, out.origin.x);
        assert_eq!(ins.origin.x, out.origin.x);
        assert_eq!(ins.size.width, px(450.0));
        assert_eq!(half.size.width, px(375.0));
    }
}
