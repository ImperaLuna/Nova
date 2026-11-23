//! A one-line text field: what is typed, a cursor, a selection, and the
//! keys every field shares. The launcher's search is the first.
//!
//! [`TextField`] is a view. Whoever owns it draws the box around it (its
//! fill, its icon, its padding) and puts the field inside as a child; the
//! field draws the text in the text style it inherits, a dim placeholder
//! while it is empty, the selection, and a steady caret in the accent while it
//! has the focus (a blinking one would redraw an open field twice a
//! second). It takes the keys that edit (the arrows, Home and End,
//! Backspace and Delete, with Ctrl by words and Shift selecting, Ctrl+A,
//! C, X and V) and lets every other key through to its parents, so the
//! owner handles Enter, Tab or the arrows up and down with a plain
//! `on_key_down` around it. Typed text and an input method's composing
//! text arrive through GPUI's input handler.
//!
//! A password's field is [`TextField::set_masked`]: it shows a dot for each
//! character, and copies and cuts nothing.
//!
//! The owner subscribes to [`FieldEvent::Changed`] and reads
//! [`TextField::text`]. [`Editing`] is the text and the cursor without
//! GPUI, tested alone.

use crate::Theme;
use crate::tokens::{CARET_WIDTH, COMPOSING_UNDERLINE, WEIGHT_BODY};
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementInputHandler, EntityInputHandler, EventEmitter,
    FocusHandle, Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Point, Render, ShapedLine, SharedString, Styled, TextAlign, TextRun,
    UTF16Selection, UnderlineStyle, Window, canvas, div, fill, point, px, size,
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// What a [`TextField`] tells its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldEvent {
    /// Its text is another one now, typed or set.
    Changed,
}

/// The text of a one-line field, and where its cursor and selection are.
/// Offsets are bytes, always between two characters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Editing {
    text: String,
    /// The selection runs from `anchor` to `cursor`, either way round.
    cursor: usize,
    anchor: usize,
    /// The most characters it holds; 0 for no limit.
    max_chars: usize,
}

impl Editing {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn selection(&self) -> Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    /// Replaces all of it, the cursor at the end.
    pub fn set(&mut self, text: &str) {
        self.text.clear();
        self.cursor = 0;
        self.anchor = 0;
        self.replace(0..0, text);
    }

    /// Puts `new` in place of `range` and the cursor after it. What a line
    /// can't hold is left out: control characters, and whatever is past the
    /// limit.
    pub fn replace(&mut self, range: Range<usize>, new: &str) {
        let range = self.boundary(range.start)..self.boundary(range.end.max(range.start));
        let kept = self.text.chars().count() - self.text[range.clone()].chars().count();
        let room = if self.max_chars == 0 { usize::MAX } else { self.max_chars.saturating_sub(kept) };
        let new: String = new.chars().filter(|character| !character.is_control()).take(room).collect();
        self.text.replace_range(range.clone(), &new);
        self.cursor = range.start + new.len();
        self.anchor = self.cursor;
    }

    /// Moves the cursor to `to`, keeping the selection's other end when
    /// `selecting`.
    pub fn move_to(&mut self, to: usize, selecting: bool) {
        self.cursor = self.boundary(to);
        if !selecting {
            self.anchor = self.cursor;
        }
    }

    pub fn select(&mut self, range: Range<usize>) {
        self.anchor = self.boundary(range.start);
        self.cursor = self.boundary(range.end);
    }

    /// An editing key: `key` as GPUI names it, with Shift (`selecting`) and
    /// Ctrl (`by_word`). Returns whether it was one.
    pub fn press(&mut self, key: &str, selecting: bool, by_word: bool) -> bool {
        let selection = self.selection();
        let collapses = !selecting && !selection.is_empty();
        match key {
            "left" if collapses && !by_word => self.move_to(selection.start, false),
            "right" if collapses && !by_word => self.move_to(selection.end, false),
            "left" => self.move_to(self.before(by_word), selecting),
            "right" => self.move_to(self.after(by_word), selecting),
            "home" => self.move_to(0, selecting),
            "end" => self.move_to(self.text.len(), selecting),
            "backspace" | "delete" if !selection.is_empty() => self.replace(selection, ""),
            "backspace" => self.replace(self.before(by_word)..self.cursor, ""),
            "delete" => self.replace(self.cursor..self.after(by_word), ""),
            "a" if by_word => self.select(0..self.text.len()),
            _ => return false,
        }
        true
    }

    /// `offset`, moved back onto the nearest character boundary.
    fn boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    /// One character, or one word, before the cursor.
    fn before(&self, by_word: bool) -> usize {
        let mut characters = self.text[..self.cursor].char_indices().rev().peekable();
        if !by_word {
            return self.text[..self.cursor].grapheme_indices(true).next_back().map_or(0, |(at, _)| at);
        }
        // Over the gap, then over the word before it.
        while characters.next_if(|(_, character)| character.is_whitespace()).is_some() {}
        let mut start = characters.peek().map_or(0, |(at, character)| at + character.len_utf8());
        while let Some((at, _)) = characters.next_if(|(_, character)| !character.is_whitespace()) {
            start = at;
        }
        start
    }

    /// One character, or one word, after the cursor.
    fn after(&self, by_word: bool) -> usize {
        let rest = &self.text[self.cursor..];
        let mut characters = rest.char_indices().peekable();
        if !by_word {
            return self.cursor + rest.graphemes(true).next().map_or(0, str::len);
        }
        while characters.next_if(|(_, character)| character.is_whitespace()).is_some() {}
        while characters.next_if(|(_, character)| !character.is_whitespace()).is_some() {}
        self.cursor + characters.peek().map_or(rest.len(), |(at, _)| *at)
    }

    fn utf16_at(&self, offset: usize) -> usize {
        self.text[..self.boundary(offset)].encode_utf16().count()
    }

    fn offset_at_utf16(&self, offset: usize) -> usize {
        let mut units = 0;
        for (at, character) in self.text.char_indices() {
            if units >= offset {
                return at;
            }
            units += character.len_utf16();
        }
        self.text.len()
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_at_utf16(range.start)..self.offset_at_utf16(range.end)
    }
}

/// What a masked field shows for each character.
const MASK: char = '•';

/// A one-line text field (see the module).
pub struct TextField {
    focus: FocusHandle,
    editing: Editing,
    placeholder: SharedString,
    /// Shown dim after the text while the cursor is at its end: what a key
    /// of the owner's would complete it to.
    ghost: SharedString,
    /// What an input method is composing.
    marked: Option<Range<usize>>,
    /// Shown as a dot per character (a password).
    masked: bool,
    /// How far the text is scrolled left, so the cursor stays in view.
    scroll: Pixels,
    selecting: bool,
    /// The text as last drawn, and where: a click lands by them.
    drawn: Option<(ShapedLine, Bounds<Pixels>)>,
}

impl EventEmitter<FieldEvent> for TextField {}

impl TextField {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            editing: Editing::default(),
            placeholder: SharedString::default(),
            ghost: SharedString::default(),
            marked: None,
            masked: false,
            scroll: px(0.0),
            selecting: false,
            drawn: None,
        }
    }

    /// The most characters it takes.
    pub fn max_chars(mut self, max_chars: usize) -> Self {
        self.editing.max_chars = max_chars;
        self
    }

    /// What gets the keyboard: hand it to the stage (`Panel::takes_typing`)
    /// or focus it yourself.
    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    pub fn text(&self) -> &str {
        self.editing.text()
    }

    /// Whether the cursor is at the end with nothing selected.
    pub fn at_end(&self) -> bool {
        self.editing.selection() == (self.text().len()..self.text().len())
    }

    /// Replaces the text, the cursor at its end.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.change(cx, |field| field.editing.set(text));
    }

    /// The dim text an empty field shows.
    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    /// Shows a dot for each character, or the text again.
    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        if masked != self.masked {
            self.masked = masked;
            cx.notify();
        }
    }

    /// Where `offset` in the text is in the line drawn: past as many dots
    /// as characters before it while masked.
    fn shown_at(&self, offset: usize) -> usize {
        match self.masked {
            true => self.text()[..offset].chars().count() * MASK.len_utf8(),
            false => offset,
        }
    }

    /// The offset in the text at `shown` in the line drawn.
    fn text_at(&self, shown: usize) -> usize {
        if !self.masked {
            return shown;
        }
        let characters = shown / MASK.len_utf8();
        self.text().char_indices().nth(characters).map_or(self.text().len(), |(offset, _)| offset)
    }

    /// The dim text after what's typed (see the field of that name).
    pub fn set_ghost(&mut self, ghost: impl Into<SharedString>, cx: &mut Context<Self>) {
        let ghost = ghost.into();
        if ghost != self.ghost {
            self.ghost = ghost;
            cx.notify();
        }
    }

    /// Runs an edit, redraws, and tells the owner when the text changed.
    fn change(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Self)) {
        let before = self.editing.text.clone();
        edit(self);
        self.marked = None;
        if self.editing.text != before {
            cx.emit(FieldEvent::Changed);
        }
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let (shift, control) = (keystroke.modifiers.shift, keystroke.modifiers.control);
        let selected = self.editing.selection();
        let clipboard = control && !shift;
        let handled = match keystroke.key.as_str() {
            "c" | "x" if clipboard => {
                if !selected.is_empty() && !self.masked {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.text()[selected.clone()].to_owned()));
                }
                if keystroke.key == "x" && !self.masked {
                    self.change(cx, |field| field.editing.replace(selected, ""));
                }
                true
            }
            "v" if clipboard => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.change(cx, |field| field.editing.replace(selected, &text));
                }
                true
            }
            key => {
                let mut edited = self.editing.clone();
                let handled = edited.press(key, shift, control);
                if handled {
                    self.change(cx, |field| field.editing = edited);
                }
                handled
            }
        };
        if handled {
            cx.stop_propagation();
        }
    }

    /// The offset in the text nearest to `position` in the window.
    fn offset_at(&self, position: Point<Pixels>) -> usize {
        let Some((line, bounds)) = &self.drawn else { return self.text().len() };
        let across = position.x - bounds.left() + self.scroll;
        self.text_at(line.closest_index_for_x(across)).min(self.text().len())
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.selecting = true;
        let at = self.offset_at(event.position);
        self.editing.move_to(at, event.modifiers.shift);
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.selecting && event.pressed_button == Some(MouseButton::Left) {
            let at = self.offset_at(event.position);
            self.editing.move_to(at, true);
            cx.notify();
        }
    }

    fn mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.selecting = false;
    }

    /// The line to draw: the text with its selection and what's being
    /// composed marked, then the ghost; or the placeholder.
    fn shape(&self, window: &mut Window, cx: &App) -> ShapedLine {
        let theme = Theme::of(cx);
        let style = window.text_style();
        let run = |len: usize, color: Hsla, background: Option<Hsla>, composing: bool| TextRun {
            len,
            font: style.font(),
            color,
            background_color: background,
            underline: composing.then_some(UnderlineStyle {
                color: Some(color),
                thickness: COMPOSING_UNDERLINE,
                wavy: false,
            }),
            strikethrough: None,
        };
        let text = self.text();
        let (shown, runs): (SharedString, Vec<TextRun>) = if text.is_empty() {
            // In the body's weight, whatever weight what's typed has.
            let mut placeholder = run(self.placeholder.len(), theme.fg_dim(), None, false);
            placeholder.font.weight = WEIGHT_BODY;
            (self.placeholder.clone(), vec![placeholder])
        } else {
            let shown = |range: Range<usize>| self.shown_at(range.start)..self.shown_at(range.end);
            let selected = shown(self.editing.selection());
            let marked = shown(self.marked.clone().unwrap_or(0..0));
            let masked: String;
            let text = match self.masked {
                true => {
                    masked = std::iter::repeat_n(MASK, text.chars().count()).collect();
                    masked.as_str()
                }
                false => text,
            };
            // Where the look changes: around the selection and the composing text.
            let mut cuts = vec![0, selected.start, selected.end, marked.start, marked.end, text.len()];
            cuts.sort_unstable();
            cuts.dedup();
            let mut runs: Vec<TextRun> = cuts
                .windows(2)
                .map(|cut| {
                    let inside = |range: &Range<usize>| range.start <= cut[0] && cut[1] <= range.end;
                    let (color, background) =
                        if inside(&selected) { (theme.on_accent, Some(theme.primary)) } else { (style.color, None) };
                    run(cut[1] - cut[0], color, background, inside(&marked))
                })
                .collect();
            let ghost = if self.at_end() { self.ghost.as_ref() } else { "" };
            runs.push(run(ghost.len(), theme.fg_dim(), None, false));
            (format!("{text}{ghost}").into(), runs)
        };
        let runs: Vec<TextRun> = runs.into_iter().filter(|run| run.len > 0).collect();
        let font_size = style.font_size.to_pixels(window.rem_size());
        window.text_system().shape_line(shown, font_size, &runs, None)
    }

    /// Draws the line in `bounds`, scrolled so the cursor is in view, with
    /// the caret when the field has the focus.
    fn paint(&mut self, bounds: Bounds<Pixels>, line: ShapedLine, window: &mut Window, cx: &mut App) {
        let cursor = line.x_for_index(self.shown_at(self.editing.cursor));
        let furthest = (bounds.size.width - CARET_WIDTH).max(px(0.0));
        // No further than the line overflows: a line that fits isn't scrolled.
        let overflow = (line.width - furthest).max(px(0.0));
        self.scroll = self.scroll.clamp((cursor - furthest).max(px(0.0)), cursor.min(overflow));
        let origin = point(bounds.left() - self.scroll, bounds.top());
        let line_height = window.line_height();
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            // A line that can't be drawn (a font went missing) leaves the field empty.
            let _ = line.paint_background(origin, line_height, TextAlign::Left, None, window, cx);
            let _ = line.paint(origin, line_height, TextAlign::Left, None, window, cx);
            if self.focus.is_focused(window) && self.editing.selection().is_empty() {
                let caret = Bounds::new(point(origin.x + cursor, bounds.top()), size(CARET_WIDTH, bounds.size.height));
                window.paint_quad(fill(caret, Theme::of(cx).primary));
            }
        });
        self.drawn = Some((line, bounds));
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.editing.range_from_utf16(&range);
        adjusted.replace(self.editing.utf16_at(range.start)..self.editing.utf16_at(range.end));
        Some(self.text()[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let selected = self.editing.selection();
        Some(UTF16Selection {
            range: self.editing.utf16_at(selected.start)..self.editing.utf16_at(selected.end),
            reversed: self.editing.cursor < self.editing.anchor,
        })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        let marked = self.marked.as_ref()?;
        Some(self.editing.utf16_at(marked.start)..self.editing.utf16_at(marked.end))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.replaced(range);
        self.change(cx, |field| field.editing.replace(range, text));
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.replaced(range);
        let start = range.start;
        let mut end = start;
        self.change(cx, |field| {
            field.editing.replace(range, new_text);
            end = field.editing.cursor;
            if let Some(selected) = new_selected {
                // Counted from the start of the new text.
                let composing = Editing { text: field.editing.text[start..end].to_owned(), ..Editing::default() };
                let selected = composing.range_from_utf16(&selected);
                field.editing.select(start + selected.start..start + selected.end);
            }
        });
        // `change` ends a composition; this one goes on while there is text.
        self.marked = (end > start).then_some(start..end);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (line, bounds) = self.drawn.as_ref()?;
        let range = self.editing.range_from_utf16(&range);
        let left = bounds.left() - self.scroll;
        Some(Bounds::from_corners(
            point(left + line.x_for_index(self.shown_at(range.start)), bounds.top()),
            point(left + line.x_for_index(self.shown_at(range.end)), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.editing.utf16_at(self.offset_at(point)))
    }
}

impl TextField {
    /// The range an input method means: the one it names, else what it is
    /// composing, else the selection.
    fn replaced(&self, range: Option<Range<usize>>) -> Range<usize> {
        range
            .map(|range| self.editing.range_from_utf16(&range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.editing.selection())
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let field = cx.entity();
        let focus = self.focus.clone();
        let text = canvas(
            {
                let field = field.clone();
                move |_, window, cx| field.read(cx).shape(window, cx)
            },
            move |bounds, line, window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, field.clone()), cx);
                field.update(cx, |field, cx| field.paint(bounds, line, window, cx));
            },
        );
        div()
            .w_full()
            .flex()
            .items_center()
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .child(text.w_full().h(window.line_height()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editing(text: &str) -> Editing {
        let mut editing = Editing::default();
        editing.set(text);
        editing
    }

    #[test]
    fn typing_goes_in_at_the_cursor_and_replaces_the_selection() {
        let mut field = editing("fire");
        field.replace(field.selection(), "fox");
        assert_eq!((field.text(), field.cursor), ("firefox", 7));
        field.select(0..4);
        field.replace(field.selection(), "water");
        assert_eq!((field.text(), field.selection()), ("waterfox", 5..5));
    }

    #[test]
    fn a_line_takes_no_control_characters_and_no_more_than_its_limit() {
        let mut field = Editing { max_chars: 5, ..Editing::default() };
        field.set("a\nb\tc");
        assert_eq!(field.text(), "abc");
        field.replace(3..3, "défg");
        assert_eq!(field.text(), "abcdé");
        field.select(0..field.text().len());
        field.replace(field.selection(), "123456789");
        assert_eq!(field.text(), "12345");
    }

    #[test]
    fn a_character_of_several_code_points_is_one_step() {
        // A family emoji (joined people) and an e with a combining accent.
        let mut field = editing("a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}e\u{301}");
        assert!(field.press("backspace", false, false));
        assert_eq!(field.text(), "a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}");
        assert!(field.press("left", false, false));
        assert_eq!(field.selection(), 1..1);
        assert!(field.press("delete", false, false));
        assert_eq!(field.text(), "a");
    }

    #[test]
    fn the_arrows_move_by_characters_and_with_ctrl_by_words() {
        let mut field = editing("né  two three");
        assert!(field.press("home", false, false));
        field.press("right", false, false);
        field.press("right", false, false);
        assert_eq!(field.cursor, 3, "past the two-byte é");
        field.press("right", false, true);
        assert_eq!(&field.text()[..field.cursor], "né  two");
        field.press("end", false, false);
        field.press("left", false, true);
        assert_eq!(&field.text()[field.cursor..], "three");
        field.press("left", false, false);
        field.press("left", false, true);
        assert_eq!(&field.text()[field.cursor..], "two three");
    }

    #[test]
    fn shift_selects_and_an_arrow_without_it_lands_on_the_selection_s_edge() {
        let mut field = editing("abcd");
        field.press("left", true, false);
        field.press("left", true, false);
        assert_eq!(field.selection(), 2..4);
        field.press("left", false, false);
        assert_eq!(field.selection(), 2..2);
        field.press("a", false, true);
        assert_eq!(field.selection(), 0..4);
        field.press("right", false, false);
        assert_eq!(field.selection(), 4..4);
    }

    #[test]
    fn backspace_and_delete_take_a_character_a_word_or_the_selection() {
        let mut field = editing("one two");
        field.press("backspace", false, false);
        assert_eq!(field.text(), "one tw");
        field.press("backspace", false, true);
        assert_eq!(field.text(), "one ");
        field.press("home", false, false);
        field.press("delete", false, false);
        assert_eq!(field.text(), "ne ");
        field.press("delete", false, true);
        assert_eq!(field.text(), " ");
        field.press("a", false, true);
        field.press("backspace", false, false);
        assert_eq!(field.text(), "");
        assert!(field.press("backspace", false, false), "still the field's key when there is nothing left");
        assert!(!field.press("enter", false, false));
        assert!(!field.press("a", false, false), "a letter is typed, not pressed");
    }

    #[test]
    fn an_input_method_counts_in_utf16() {
        let field = editing("e🙂漢");
        assert_eq!(field.utf16_at(field.text().len()), 4);
        assert_eq!(field.range_from_utf16(&(1..3)), 1..5);
        assert_eq!(field.range_from_utf16(&(3..99)), 5..8);
    }
}
