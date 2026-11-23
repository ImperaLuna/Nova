//! How wide a line of text is and how many lines it wraps to, worked out
//! before it is drawn: the pill's width eases to its content's, and the
//! panel's height is asked of the stage before the panel opens. The same
//! text system lays the text out when it is drawn, so the two agree.

use gpui::{App, Font, FontWeight, LineFragment, LineLayout, Pixels, TextRun, Window, font};
use nova_look::Theme;
use nova_look::tokens::FONT_SANS;
use std::sync::Arc;

/// `text` laid out on one line at `size` and `weight` in Nova's face, as
/// `window` will draw it.
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

/// How wide `text` is at `size` and `weight`, from its letters' widths
/// alone: for a size asked before a window draws it (kerning aside, the
/// same).
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

/// How many lines `text` takes at `wrap` wide, at most `most`; one for no
/// text, as a text item is a line tall when empty.
pub fn lines(text: &str, (size, weight): (Pixels, FontWeight), wrap: Pixels, most: usize, cx: &App) -> usize {
    let mut wrapper = cx.text_system().line_wrapper(face(weight), size);
    let fragments = [LineFragment::text(text)];
    let breaks = wrapper.wrap_line(&fragments, wrap, gpui::IndentAdjustment::NoIndent).count();
    (breaks + 1).min(most)
}
