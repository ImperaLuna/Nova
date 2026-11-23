//! A thick slider: a round icon button beside the track (a click on it mutes),
//! the track filled only up to the value with a label and the percent on it,
//! and an arrow button after it that opens a page. Drag or click the track to
//! set it. Quick settings' volume and mic sliders and the Sound page's app
//! rows.
//!
//! It keeps one thing of its own, by its id: while the pointer holds it,
//! the value under the pointer. The value set comes back from novad a
//! moment later, and drawing that instead would jump behind the pointer.

use crate::style::LookStyled as _;
use crate::tokens::{
    DIM_OPACITY, FG_DIM_OPACITY, FONT_MONO, FONT_SIZE_SMALL, GAP, PAD, SLIDER_APP_MUTED_OPACITY, SLIDER_APP_RADIUS,
    SLIDER_BUTTON_WIDTH, SLIDER_FILL_OPACITY, SLIDER_GAP, SLIDER_HEIGHT, SLIDER_MUTED_OPACITY, WEIGHT_TITLE,
};
use crate::{AppIcon, Glyph, Icon, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Bounds, ClickEvent, Context, DragMoveEvent, ElementId, Entity, Hsla,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point, Render, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, canvas, div, relative,
};
use std::cell::Cell;
use std::rc::Rc;

type Clicked = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type Moved = Rc<dyn Fn(f32, &mut Window, &mut App)>;

/// What the round button beside the track shows.
#[derive(Clone)]
enum Lead {
    /// A line icon, in the slider's icon color (dim while muted).
    Icon(Icon),
    /// An app's icon file, or its name's first letter without one.
    App { id: SharedString, icon: Option<SharedString> },
}

/// A thick slider (see the module).
#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f32,
    label: SharedString,
    lead: Lead,
    icon_color: Option<Hsla>,
    fill: Option<Hsla>,
    available: bool,
    muted: bool,
    on_move: Option<Moved>,
    on_icon: Option<Clicked>,
    on_open: Option<Clicked>,
}

impl Slider {
    /// A slider at `value` (0 to 1; more shows full) with a line icon.
    pub fn new(id: impl Into<ElementId>, icon: Icon, value: f32, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            value,
            label: label.into(),
            lead: Lead::Icon(icon),
            icon_color: None,
            fill: None,
            available: true,
            muted: false,
            on_move: None,
            on_icon: None,
            on_open: None,
        }
    }

    /// An app's slider: its icon (an absolute path, as novad sends it), or
    /// the first letter of `label`; `id` picks nothing but the element.
    pub fn app(id: impl Into<ElementId>, app: impl Into<SharedString>, icon: Option<SharedString>, value: f32) -> Self {
        let app = app.into();
        let lead = Lead::App { id: app.clone(), icon };
        Self { lead, ..Self::new(id, Icon::new(Glyph::STAR), value, app) }
    }

    /// The icon's color (the ink by default), and the track's fill (the
    /// ink, faint, by default): the mic slider is the second accent.
    pub fn colors(mut self, icon: Hsla, fill: Hsla) -> Self {
        self.icon_color = Some(icon);
        self.fill = Some(fill);
        self
    }

    /// `false` dims it and takes no input (no output to set).
    pub fn available(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    /// Muted: the fill stays at the volume, faint, and the icon dims.
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// Called with the new value (to 1/100) as the pointer sets it.
    pub fn on_move(mut self, moved: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_move = Some(Rc::new(moved));
        self
    }

    /// A click on the icon (mute).
    pub fn on_icon(mut self, clicked: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_icon = Some(Rc::new(clicked));
        self
    }

    /// Gives it the arrow button after the track, which opens a page.
    pub fn on_open(mut self, opened: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_open = Some(Rc::new(opened));
        self
    }
}

/// What a slider keeps between frames: the value under the pointer while
/// it holds the track, and where the track was last drawn.
#[derive(Default)]
struct Held {
    value: Option<f32>,
    track: Rc<Cell<Bounds<Pixels>>>,
}

/// What a drag of a slider carries: the slider's state, so only it moves.
struct Dragging(Entity<Held>);

/// The drag's picture: nothing, the fill follows the pointer instead.
struct NoPicture;

impl Render for NoPicture {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The value at `x` on a track at `bounds`, to 1/100.
fn value_at(x: Pixels, bounds: Bounds<Pixels>) -> f32 {
    if bounds.size.width <= Pixels::ZERO {
        return 0.0;
    }
    let share = ((x - bounds.left()) / bounds.size.width).clamp(0.0, 1.0);
    (share * 100.0).round() / 100.0
}

/// Sets the held value from the pointer at `at`, and returns it to send:
/// always on a press, after that only when it changed.
fn hold(held: &Entity<Held>, at: Point<Pixels>, pressed: bool, cx: &mut App) -> Option<f32> {
    let next = value_at(at.x, held.read(cx).track.get());
    if !pressed && held.read(cx).value == Some(next) {
        return None;
    }
    held.update(cx, |held, cx| {
        held.value = Some(next);
        cx.notify();
    });
    Some(next)
}

fn let_go(held: &Entity<Held>, cx: &mut App) {
    held.update(cx, |held, cx| {
        if held.value.take().is_some() {
            cx.notify();
        }
    });
}

/// Sends `value` to the slider's owner, when it has one and it changed.
fn send(moved: Option<&Moved>, value: Option<f32>, window: &mut Window, cx: &mut App) {
    if let (Some(moved), Some(value)) = (moved, value) {
        moved(value, window, cx);
    }
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let held = window.use_keyed_state(self.id.clone(), cx, |_, _| Held::default());
        let current = held.read(cx).value.unwrap_or(self.value);
        let available = self.available;
        div()
            .id(self.id.clone())
            .w_full()
            .h(SLIDER_HEIGHT)
            .flex()
            .gap(SLIDER_GAP)
            .body_text(cx)
            .when(!available, |slider| slider.opacity(DIM_OPACITY))
            .child(self.lead_button(cx))
            .child(self.track(&held, current, cx))
            .children(self.arrow(cx))
    }
}

impl Slider {
    /// The round button before the track, with its icon.
    fn lead_button(&self, cx: &App) -> impl IntoElement + use<> {
        let theme = Theme::of(cx);
        let (available, muted, hover) = (self.available, self.muted, theme.hover());
        let ink = if muted { theme.fg.alpha(FG_DIM_OPACITY) } else { self.icon_color.unwrap_or(theme.fg) };
        let lead: AnyElement = match self.lead.clone() {
            Lead::Icon(icon) => icon.color(ink).into_any_element(),
            Lead::App { id, icon: Some(icon) } => div()
                .when(muted, |lead| lead.opacity(SLIDER_APP_MUTED_OPACITY))
                .child(AppIcon::new(id, self.label.clone()).icon(Some(icon)).size(SLIDER_HEIGHT))
                .into_any_element(),
            Lead::App { icon: None, .. } => div()
                .text_size(FONT_SIZE_SMALL)
                .font_weight(WEIGHT_TITLE)
                .text_color(if muted { theme.fg_dim() } else { theme.fg })
                .child(first_letter(&self.label))
                .into_any_element(),
        };
        let has_picture = matches!(self.lead, Lead::App { icon: Some(_), .. });
        let is_app = matches!(self.lead, Lead::App { .. });
        div()
            .id("icon")
            .flex_none()
            .size(SLIDER_HEIGHT)
            .flex()
            .items_center()
            .justify_center()
            .map(|button| if is_app { button.rounded(SLIDER_APP_RADIUS) } else { button.rounded_full() })
            .when(!has_picture, |button| {
                button.bg(theme.track()).when(available, |button| button.hover(move |style| style.bg(hover)))
            })
            .when_some(self.on_icon.clone().filter(|_| available), |button, clicked| {
                button.on_click(move |event, window, cx| clicked(event, window, cx))
            })
            .child(lead)
    }

    /// The track, filled to `current`, with the label and the percent on
    /// it; the pointer sets it.
    fn track(&self, held: &Entity<Held>, current: f32, cx: &App) -> impl IntoElement + use<> {
        let theme = Theme::of(cx);
        let (available, muted) = (self.available, self.muted);
        let fill = self.fill.unwrap_or(theme.fg.alpha(SLIDER_FILL_OPACITY));
        let track_box = held.read(cx).track.clone();
        let filled = div()
            .absolute()
            .top_0()
            .left_0()
            .h_full()
            .w(relative(current.clamp(0.0, 1.0)))
            .rounded_full()
            .bg(fill)
            .when(muted, |fill| fill.opacity(SLIDER_MUTED_OPACITY));
        let words = div()
            .absolute()
            .size_full()
            .px(PAD)
            .flex()
            .items_center()
            .gap(GAP)
            .text_size(FONT_SIZE_SMALL)
            .child(div().flex_1().min_w_0().truncate().child(self.label.clone()))
            .when(available, |row| {
                row.child(
                    div()
                        .flex_none()
                        .font_family(FONT_MONO)
                        .when(muted, |number| number.text_color(theme.fg_dim()))
                        .child(format!("{}", (current * 100.0).round() as i64)),
                )
            });
        let track = div()
            .id("track")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .rounded_full()
            .bg(theme.track())
            .overflow_hidden()
            .child(canvas(move |bounds, _, _| track_box.set(bounds), |_, (), _, _| {}).absolute().size_full())
            .child(filled)
            .child(words);
        if !available {
            return track;
        }
        let (moved, moved_on) = (self.on_move.clone(), self.on_move.clone());
        let (pressed, dragged, released, released_out) = (held.clone(), held.clone(), held.clone(), held.clone());
        track
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                let value = hold(&pressed, event.position, true, cx);
                send(moved.as_ref(), value, window, cx);
            })
            .on_drag(Dragging(held.clone()), |_, _, _, cx| cx.new(|_| NoPicture))
            .on_drag_move(move |event: &DragMoveEvent<Dragging>, window, cx| {
                if event.drag(cx).0 == dragged {
                    let value = hold(&dragged, event.event.position, false, cx);
                    send(moved_on.as_ref(), value, window, cx);
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| let_go(&released, cx))
            .on_mouse_up_out(MouseButton::Left, move |_, _, cx| let_go(&released_out, cx))
    }

    /// The button after the track that opens a page, when it has one.
    fn arrow(&self, cx: &App) -> Option<impl IntoElement + use<>> {
        let theme = Theme::of(cx);
        let hover = theme.hover();
        self.on_open.clone().map(|opened| {
            div()
                .id("open")
                .flex_none()
                .w(SLIDER_BUTTON_WIDTH)
                .h_full()
                .rounded_full()
                .bg(theme.track())
                .hover(move |style| style.bg(hover))
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(Glyph::RIGHT))
                .on_click(move |event, window, cx| opened(event, window, cx))
        })
    }
}

/// A name's first letter, upper case.
fn first_letter(name: &str) -> String {
    name.chars().next().map(|letter| letter.to_uppercase().collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    #[test]
    fn the_value_is_where_the_pointer_is_on_the_track_to_a_hundredth() {
        let track = Bounds { origin: point(px(100.0), px(0.0)), size: size(px(200.0), px(34.0)) };
        assert_eq!(value_at(px(100.0), track), 0.0);
        assert_eq!(value_at(px(200.0), track), 0.5);
        assert_eq!(value_at(px(301.0), track), 1.0, "past the end is full");
        assert_eq!(value_at(px(50.0), track), 0.0, "before the start is empty");
        assert_eq!(value_at(px(133.0), track), 0.17);
        let nothing = Bounds { origin: point(px(0.0), px(0.0)), size: size(px(0.0), px(0.0)) };
        assert_eq!(value_at(px(10.0), nothing), 0.0, "a track not drawn yet");
    }

    #[test]
    fn an_app_without_an_icon_shows_its_letter() {
        assert_eq!(first_letter("firefox"), "F");
        assert_eq!(first_letter(""), "");
    }
}
