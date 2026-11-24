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

const ELLIPSIS: &str = "…";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldEvent {
    Changed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Editing {
    text: String,
    cursor: usize,
    anchor: usize,
    max_chars: usize,
}

impl Editing {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn selection(&self) -> Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    pub fn set(&mut self, text: &str) {
        self.text.clear();
        self.cursor = 0;
        self.anchor = 0;
        self.replace(0..0, text);
    }

    pub fn replace(&mut self, range: Range<usize>, new: &str) {
        let range = self.boundary(range.start)..self.boundary(range.end.max(range.start));
        let kept = self.text.chars().count() - self.text[range.clone()].chars().count();
        let room = if self.max_chars == 0 { usize::MAX } else { self.max_chars.saturating_sub(kept) };
        let new: String = new.chars().filter(|character| !character.is_control()).take(room).collect();
        self.text.replace_range(range.clone(), &new);
        self.cursor = range.start + new.len();
        self.anchor = self.cursor;
    }

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

    fn boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    fn before(&self, by_word: bool) -> usize {
        let mut characters = self.text[..self.cursor].char_indices().rev().peekable();
        if !by_word {
            return self.text[..self.cursor].grapheme_indices(true).next_back().map_or(0, |(at, _)| at);
        }
        while characters.next_if(|(_, character)| character.is_whitespace()).is_some() {}
        let mut start = characters.peek().map_or(0, |(at, character)| at + character.len_utf8());
        while let Some((at, _)) = characters.next_if(|(_, character)| !character.is_whitespace()) {
            start = at;
        }
        start
    }

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

const MASK: char = '•';

pub struct TextField {
    focus: FocusHandle,
    editing: Editing,
    placeholder: SharedString,
    ghost: SharedString,
    marked: Option<Range<usize>>,
    masked: bool,
    scroll: Pixels,
    selecting: bool,
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

    pub fn max_chars(mut self, max_chars: usize) -> Self {
        self.editing.max_chars = max_chars;
        self
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    pub fn text(&self) -> &str {
        self.editing.text()
    }

    pub fn at_end(&self) -> bool {
        self.editing.selection() == (self.text().len()..self.text().len())
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.change(cx, |field| field.editing.set(text));
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        if masked != self.masked {
            self.masked = masked;
            cx.notify();
        }
    }

    fn shown_at(&self, offset: usize) -> usize {
        match self.masked {
            true => self.text()[..offset].chars().count() * MASK.len_utf8(),
            false => offset,
        }
    }

    fn text_at(&self, shown: usize) -> usize {
        if !self.masked {
            return shown;
        }
        let characters = shown / MASK.len_utf8();
        self.text().char_indices().nth(characters).map_or(self.text().len(), |(offset, _)| offset)
    }

    pub fn set_ghost(&mut self, ghost: impl Into<SharedString>, cx: &mut Context<Self>) {
        let ghost = ghost.into();
        if ghost != self.ghost {
            self.ghost = ghost;
            cx.notify();
        }
    }

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

    fn shape(&self, width: Pixels, window: &mut Window, cx: &App) -> ShapedLine {
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
            let font_size = style.font_size.to_pixels(window.rem_size());
            let ghost_run = |len: usize| run(len, theme.fg_dim(), None, false);
            let ghost = fit_ghost(text, ghost, width, |line: &str| {
                let mut measured = runs.clone();
                measured.push(ghost_run(line.len() - text.len()));
                measured.retain(|run| run.len > 0);
                window.text_system().shape_line(line.to_owned().into(), font_size, &measured, None).width
            });
            runs.push(ghost_run(ghost.len()));
            (format!("{text}{ghost}").into(), runs)
        };
        let runs: Vec<TextRun> = runs.into_iter().filter(|run| run.len > 0).collect();
        let font_size = style.font_size.to_pixels(window.rem_size());
        window.text_system().shape_line(shown, font_size, &runs, None)
    }

    fn paint(&mut self, bounds: Bounds<Pixels>, line: ShapedLine, window: &mut Window, cx: &mut App) {
        let cursor = line.x_for_index(self.shown_at(self.editing.cursor));
        let furthest = (bounds.size.width - CARET_WIDTH).max(px(0.0));
        let overflow = (line.width - furthest).max(px(0.0));
        self.scroll = self.scroll.clamp((cursor - furthest).max(px(0.0)), cursor.min(overflow));
        let origin = point(bounds.left() - self.scroll, bounds.top());
        let line_height = window.line_height();
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
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

fn fit_ghost(text: &str, ghost: &str, width: Pixels, measure: impl Fn(&str) -> Pixels) -> String {
    let fits = |line: &str| measure(line) <= width;
    if ghost.is_empty() || fits(&format!("{text}{ghost}")) || !fits(text) {
        return ghost.to_owned();
    }
    let ends: Vec<usize> = ghost.char_indices().map(|(at, _)| at).collect();
    let shortened = |count: usize| format!("{}{ELLIPSIS}", &ghost[..ends[count]]);
    let kept = ends.partition_point(|&end| fits(&format!("{text}{}{ELLIPSIS}", &ghost[..end])));
    if kept == 0 { String::new() } else { shortened(kept - 1) }
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
        if self.masked {
            return Some(MASK.to_string().repeat(self.text()[range].chars().count()));
        }
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
                let composing = Editing { text: field.editing.text[start..end].to_owned(), ..Editing::default() };
                let selected = composing.range_from_utf16(&selected);
                field.editing.select(start + selected.start..start + selected.end);
            }
        });
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
                move |bounds: Bounds<Pixels>, window, cx| field.read(cx).shape(bounds.size.width, window, cx)
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

    #[test]
    fn a_completion_too_long_for_the_field_ends_in_an_ellipsis() {
        let measure = |line: &str| px(10.0 * line.chars().count() as f32);
        let fit = |text: &str, ghost: &str, chars: f32| fit_ghost(text, ghost, px(10.0 * chars), measure);
        assert_eq!(fit("~/do", "cuments/", 12.0), "cuments/");
        assert_eq!(fit("~/do", "cuments/", 10.0), "cumen…");
        assert_eq!(fit("~/do", "cuments/", 5.0), "…");
        assert_eq!(fit("~/do", "cuments/", 4.0), "");
        assert_eq!(fit("~/documents", "/", 4.0), "/", "a field already scrolled keeps it");
    }

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
