use gpui::{App, Font, FontWeight, LineFragment, LineLayout, Pixels, TextRun, Window, font};
use nova_look::Theme;
use nova_look::tokens::FONT_SANS;
use std::sync::Arc;

pub fn line(text: &str, size: Pixels, weight: FontWeight, window: &Window, cx: &App) -> Arc<LineLayout> {
    let run = TextRun {
        len: text.len(),
        font: face(weight),
        color: Theme::of(cx).fg,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().layout_line(text, size, &[run], None)
}

pub fn width(text: &str, size: Pixels, weight: FontWeight, cx: &App) -> Pixels {
    let system = cx.text_system();
    let id = system.resolve_font(&face(weight));
    text.chars().map(|letter| system.layout_width(id, size, letter)).fold(Pixels::ZERO, |sum, width| sum + width)
}

fn face(weight: FontWeight) -> Font {
    let mut face = font(FONT_SANS);
    face.weight = weight;
    face
}

pub fn lines(text: &str, (size, weight): (Pixels, FontWeight), wrap: Pixels, most: usize, cx: &App) -> usize {
    let mut wrapper = cx.text_system().line_wrapper(face(weight), size);
    let fragments = [LineFragment::text(text)];
    let breaks = wrapper.wrap_line(&fragments, wrap, gpui::IndentAdjustment::NoIndent).count();
    (breaks + 1).min(most)
}
