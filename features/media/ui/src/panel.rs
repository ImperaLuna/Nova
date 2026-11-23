//! The now-playing panel's content:
//! the app's name in the header row, which closes the panel; the cover
//! beside the title, artist and album; the progress line to seek on with
//! the times; the buttons; a chip per player to switch between them when
//! there are two; and a row going to the player's window.
//!
//! The stage grows the pill into it and fades it in. Its size is the
//! model's ([`NowPlaying`] asks the stage), worked out from the same text.

use crate::art::Art;
use crate::logic;
use crate::model::{self, NAME, NowPlaying, TITLE_LINES};
use crate::tokens::{
    BODY_GAP, CHIP_DOT, CHIP_GAP, CHIP_HEIGHT, CHIP_PAD, CONTROL, CONTROL_GAP, CONTROL_ICON, COVER, DISABLED_OPACITY,
    DURATION_SEEK_HOLD, GAP, GO_TO_ICON, KNOB, META_GAP, PLAY, PLAY_ICON, ROW_GAP, SEEK_ROOM, TIMES_GAP, TITLE_LINE,
    TRACK, TRACK_HOVER,
};
use gpui::DisplayId;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Div, Entity, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, SharedString, Styled, Task,
    WeakEntity, Window, canvas, div, relative,
};
use nova_look::tokens::{
    BAR_HEIGHT, FONT_MONO, FONT_SIZE_SMALL, FONT_SIZE_TITLE, ICON_GAP, ICON_SIZE_SMALL, LINE_HEIGHT, PANEL_PAD,
    RADIUS_CARD, RADIUS_ROW, ROW_HEIGHT, ROW_INSET, SECTION_GAP, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{Glyph, Icon, Theme};
use nova_media_api::{MediaAction, MediaPlayer};
use nova_stage::Stage;
use std::cell::Cell;
use std::rc::Rc;

/// What hovering the progress line and the go-to row shows.
const SEEK_GROUP: &str = "media-seek";
const GO_TO_GROUP: &str = "media-go-to";

pub struct MediaPanel {
    model: Entity<NowPlaying>,
    /// Where a drag or a click put the line (0..1), until the player says
    /// where it is, or a while passes without word: a seek that failed.
    held: Option<f32>,
    pressing: bool,
    let_go: Option<Task<()>>,
    /// Where the progress line was laid out, for a click's fraction.
    line: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl MediaPanel {
    pub fn new(model: Entity<NowPlaying>, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        // The players' next word ends a held position; the model's own
        // redraws (the time ticking) don't.
        let feed = model.read(cx).feed().clone();
        cx.observe(&feed, |panel, _, cx| {
            if !panel.pressing && panel.held.take().is_some() {
                cx.notify();
            }
        })
        .detach();
        Self { model, held: None, pressing: false, let_go: None, line: Rc::default() }
    }

    fn fraction_at(&self, x: Pixels) -> f32 {
        let Some(line) = self.line.get() else { return 0.0 };
        if line.size.width <= Pixels::ZERO {
            return 0.0;
        }
        ((x - line.left()) / line.size.width).clamp(0.0, 1.0)
    }

    fn press(&mut self, x: Pixels, cx: &mut Context<Self>) {
        self.let_go = None;
        self.pressing = true;
        self.held = Some(self.fraction_at(x));
        cx.notify();
    }

    fn drag(&mut self, x: Pixels, cx: &mut Context<Self>) {
        if self.pressing {
            self.held = Some(self.fraction_at(x));
            cx.notify();
        }
    }

    /// Lets go of the line: seeks there, and holds it there a while.
    fn release(&mut self, x: Pixels, cx: &mut Context<Self>) {
        if !self.pressing {
            return;
        }
        self.pressing = false;
        let fraction = self.fraction_at(x);
        self.held = Some(fraction);
        let model = self.model.read(cx);
        if let Some(player) = model.current() {
            model.seek(logic::seek_target(player, fraction), cx);
        }
        self.let_go = Some(cx.spawn(async move |panel, cx| {
            cx.background_executor().timer(DURATION_SEEK_HOLD).await;
            let _ = panel.update(cx, |panel, cx| {
                panel.held = None;
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for MediaPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let Some(player) = model.current().cloned() else { return div().size_full() };
        let workspace = model.workspace.clone();
        let chips: Vec<MediaPlayer> =
            model.media(cx).map(|media| logic::chips(media).into_iter().cloned().collect()).unwrap_or_default();
        let theme = Theme::of(cx);
        let dim = theme.fg_dim();
        let screen = model.screen.id;

        let header = div()
            .h(BAR_HEIGHT)
            .px(PANEL_PAD)
            .flex()
            .items_center()
            .gap(ICON_GAP)
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _, cx: &mut App| Stage::close(NAME, screen, cx))
            .child(Art::icon(ICON_SIZE_SMALL, ICON_SIZE_SMALL).build(logic::app_icon(&player), cx))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(WEIGHT_LABEL)
                    .text_color(dim)
                    .child(SharedString::from(logic::app_name(&player).to_owned())),
            );
        let content = div()
            .px(PANEL_PAD)
            .pt(GAP)
            .flex()
            .flex_col()
            .child(body(&player, cx))
            .child(div().h(SECTION_GAP).flex_none())
            .child(self.seek_line(&player, cx))
            .child(div().h(GAP).flex_none())
            .child(controls(&self.model, &player, cx))
            .when(!chips.is_empty(), |content| {
                content.child(div().h(ROW_GAP).flex_none()).child(chip_row(&self.model, &player, chips, cx))
            })
            .child(div().h(ROW_GAP).flex_none())
            .child(go_to(&player, &workspace, screen, cx));
        div().size_full().flex().flex_col().child(header).child(content)
    }
}

/// The cover beside the title (two lines at most), the artist and the
/// album.
fn body(player: &MediaPlayer, cx: &App) -> Div {
    let dim = Theme::of(cx).fg_dim();
    let line = |text: &str| SharedString::from(text.to_owned());
    let title = div()
        .w_full()
        .text_size(FONT_SIZE_TITLE)
        .font_weight(WEIGHT_TITLE)
        // The title's two lines sit a little wider apart.
        .line_height(relative(LINE_HEIGHT * TITLE_LINE))
        .line_clamp(TITLE_LINES)
        .text_ellipsis()
        .child(line(&player.title));
    let artist =
        (!player.artist.is_empty()).then(|| div().w_full().truncate().text_color(dim).child(line(&player.artist)));
    let album = (!player.album.is_empty())
        .then(|| div().w_full().truncate().text_size(FONT_SIZE_SMALL).text_color(dim).child(line(&player.album)));
    let meta = div().flex_1().min_w_0().flex().flex_col().gap(META_GAP).child(title).children(artist).children(album);
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(BODY_GAP)
        .child(Art::cover(COVER, RADIUS_CARD).build(player.cover.as_deref(), cx))
        .child(meta)
}

impl MediaPanel {
    /// The progress line with the times under it. Clicking or dragging on
    /// it seeks; hovered, it grows and shows its knob. A player that can't
    /// seek, or a track of unknown length, shows the line without taking
    /// clicks.
    fn seek_line(&self, player: &MediaPlayer, cx: &mut Context<Self>) -> Div {
        let theme = Theme::of(cx);
        let (dim, track, meter, fg) = (theme.fg_dim(), theme.track(), theme.meter(), theme.fg);
        let seekable = logic::seekable(player);
        let now = model::now_ms();
        let fraction = self.held.unwrap_or_else(|| logic::progress(player, now));
        let lifted = self.pressing;
        let around = (SEEK_ROOM - TRACK) / 2.0;
        let (rest_top, lifted_top) = ((SEEK_ROOM - TRACK) / 2.0, (SEEK_ROOM - TRACK_HOVER) / 2.0);
        let mut line = div()
            .absolute()
            .left_0()
            .right_0()
            .top(if lifted { lifted_top } else { rest_top })
            .h(if lifted { TRACK_HOVER } else { TRACK })
            .rounded_full()
            .bg(track)
            .child(div().h_full().w(relative(fraction)).rounded_full().bg(meter));
        let mut knob = div()
            .absolute()
            .top((SEEK_ROOM - KNOB) / 2.0)
            .left(relative(fraction))
            .ml(-KNOB / 2.0)
            .size(KNOB)
            .rounded_full()
            .bg(fg)
            .opacity(if lifted && seekable { 1.0 } else { 0.0 });
        if seekable {
            line = line.group_hover(SEEK_GROUP, move |style| style.top(lifted_top).h(TRACK_HOVER));
            knob = knob.group_hover(SEEK_GROUP, |style| style.opacity(1.0));
        }
        let bounds = self.line.clone();
        let mut area = div()
            .id("media-seek")
            .group(SEEK_GROUP)
            .relative()
            .w_full()
            .h(SEEK_ROOM)
            .my(-around)
            .child(canvas(move |found, _, _| bounds.set(Some(found)), |_, (), _, _| {}).absolute().inset_0())
            .child(line)
            .child(knob);
        if seekable {
            // A drag follows the pointer anywhere on the window, and ends
            // wherever the button goes up. Listened for before any press,
            // so a quick click's release is never missed.
            let panel = cx.weak_entity();
            area = area
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|panel, event: &MouseDownEvent, _, cx| panel.press(event.position.x, cx)),
                )
                .child(canvas(|_, _, _| {}, move |_, (), window, _| follow_drag(panel, window)).absolute().size_0());
        }
        let at = match self.held {
            Some(held) => logic::seek_target(player, held),
            None => logic::position(player, now),
        };
        let length = (player.length_us > 0).then(|| SharedString::from(logic::time(player.length_us)));
        let times = div()
            .mt(TIMES_GAP)
            .flex()
            .justify_between()
            .font_family(FONT_MONO)
            .text_size(FONT_SIZE_SMALL)
            .text_color(dim)
            .child(SharedString::from(logic::time(at)))
            .children(length);
        div().w_full().flex().flex_col().child(area).child(times)
    }
}

/// Follows a drag on the progress line anywhere on the window until the
/// button goes up. Moves and releases without a press do nothing.
fn follow_drag(panel: WeakEntity<MediaPanel>, window: &mut Window) {
    let released = panel.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            let _ = panel.update(cx, |panel, cx| panel.drag(event.position.x, cx));
        }
    });
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            let _ = released.update(cx, |panel, cx| panel.release(event.position.x, cx));
        }
    });
}

/// A round button: the previous and next ones on a hover wash, play and
/// pause filled with the accent.
fn round_button(icon: Icon, size: Pixels, filled: bool, enabled: bool, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let (primary, accent, hover) = (theme.primary, theme.accent_fill(), theme.hover());
    let button = div().size(size).flex_none().rounded_full().flex().items_center().justify_center().child(icon);
    let button = if filled { button.bg(accent) } else { button };
    if !enabled {
        return button.opacity(DISABLED_OPACITY);
    }
    let lit = if filled { primary } else { hover };
    button.cursor_pointer().hover(move |style| style.bg(lit))
}

fn controls(model: &Entity<NowPlaying>, player: &MediaPlayer, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let (fg, on_accent) = (theme.fg, theme.on_accent);
    let playing = logic::playing(Some(player));
    let button = |icon: Glyph, fill: Glyph, action: MediaAction, enabled: bool| {
        let model = model.clone();
        round_button(Icon::new(icon).filled(fill).size(CONTROL_ICON).color(fg), CONTROL, false, enabled, cx).when(
            enabled,
            move |button| {
                button.on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _, cx: &mut App| {
                    model.read(cx).control(action, cx);
                })
            },
        )
    };
    let solid = if playing { Glyph::PAUSE_SOLID } else { Glyph::PLAY_SOLID };
    let can_play = if playing { player.can_pause } else { player.can_play };
    let play =
        {
            let model = model.clone();
            round_button(Icon::new(solid).filled(solid).size(PLAY_ICON).color(on_accent), PLAY, true, can_play, cx)
                .when(can_play, move |button| {
                    button.on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _, cx: &mut App| {
                        model.read(cx).control(MediaAction::PlayPause, cx);
                    })
                })
        };
    div()
        .flex()
        .justify_center()
        .items_center()
        .gap(CONTROL_GAP)
        .child(button(Glyph::SKIP_PREVIOUS, Glyph::SKIP_PREVIOUS_FILL, MediaAction::Previous, player.can_previous))
        .child(play)
        .child(button(Glyph::SKIP_NEXT, Glyph::SKIP_NEXT_FILL, MediaAction::Next, player.can_next))
}

/// One chip per player to show it instead: the chosen one solid, a dot on
/// the ones that play.
fn chip_row(model: &Entity<NowPlaying>, current: &MediaPlayer, chips: Vec<MediaPlayer>, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let (fg, dim, primary, on_accent, raised, selected) =
        (theme.fg, theme.fg_dim(), theme.primary, theme.on_accent, theme.surface_raised(), theme.selected());
    let chips: Vec<AnyElement> = chips
        .into_iter()
        .map(|chip| {
            let chosen = chip.id == current.id;
            let dot = logic::playing(Some(&chip))
                .then(|| div().size(CHIP_DOT).rounded_full().bg(if chosen { on_accent } else { primary }));
            let name = div()
                .text_size(FONT_SIZE_SMALL)
                .font_weight(WEIGHT_LABEL)
                .text_color(if chosen { on_accent } else { dim })
                .child(SharedString::from(logic::app_name(&chip).to_owned()));
            let (model, id) = (model.clone(), chip.id.clone());
            let face = div()
                .h(CHIP_HEIGHT)
                .px(CHIP_PAD)
                .flex_none()
                .rounded_full()
                .flex()
                .items_center()
                .gap(ICON_GAP)
                .cursor_pointer()
                .bg(if chosen { fg } else { raised })
                .children(dot)
                .child(name)
                .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _, cx: &mut App| {
                    model.read(cx).show(&id, cx);
                });
            let face = if chosen { face } else { face.hover(move |style| style.bg(selected)) };
            face.into_any_element()
        })
        .collect();
    div().w_full().flex().flex_wrap().gap(CHIP_GAP).children(chips)
}

/// Brings the player's window forward, then closes.
fn go_to(player: &MediaPlayer, workspace: &str, screen: DisplayId, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let (fg, dim, raised, selected) = (theme.fg, theme.fg_dim(), theme.surface_raised(), theme.selected());
    let name = logic::app_name(player);
    let where_ = (!workspace.is_empty()).then(|| {
        div()
            .min_w_0()
            .truncate()
            .text_size(FONT_SIZE_SMALL)
            .text_color(dim)
            .child(SharedString::from(format!("workspace {workspace}")))
    });
    let id = player.id.clone();
    div()
        .group(GO_TO_GROUP)
        .mx(-ROW_INSET)
        .h(ROW_HEIGHT)
        .px(ROW_INSET)
        .rounded(RADIUS_ROW)
        .bg(raised)
        .hover(move |style| style.bg(selected))
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(ROW_INSET)
        .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _, cx: &mut App| model::go_to(&id, screen, cx))
        .child(Art::icon(GO_TO_ICON, GO_TO_ICON).build(logic::app_icon(player), cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(WEIGHT_LABEL)
                .child(SharedString::from(format!("Go to {name}"))),
        )
        .children(where_)
        .child(
            // Lit while the row is hovered: the bright arrow over the dim one.
            div().relative().flex_none().child(Icon::new(Glyph::GO_TO).size(ICON_SIZE_SMALL).color(dim)).child(
                div()
                    .absolute()
                    .inset_0()
                    .opacity(0.0)
                    .group_hover(GO_TO_GROUP, |style| style.opacity(1.0))
                    .child(Icon::new(Glyph::GO_TO).size(ICON_SIZE_SMALL).color(fg)),
            ),
        )
}
