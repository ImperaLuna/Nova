//! Drawing the carousel: the preview over the whole screen with a shade
//! under the deck, the deck of cards, the label with the screen picker
//! under it, and the mark on the screen picked for.
//!
//! Every card is laid out by the deck's numbers (`deck.rs`), not by GPUI:
//! only the cards near the middle are built, the furthest first so nearer
//! ones lie over them. `card.rs` draws one, turned.

use crate::card::{CardLook, ChipPlaces, draw_card};
use crate::carousel::{Carousel, NAME};
use crate::deck::{file_name, offset, shown};
use crate::images::Wanted;
use crate::tilt::Tilt;
use crate::tokens::{
    CARD_SHARE, CHIP_HEIGHT, DEPTH_SCALE, ENTER_RISE, ENTER_SPREAD, LABEL_GAP, LABEL_HEIGHT, LABEL_PAD, LEGEND_DOT,
    LOADING, MINI_GAP, MINI_IDLE, MINI_RADIUS, MINI_RING, MINI_WIDTH, NEIGHBOUR, NOTE_WIDTH, PICKER_HEIGHT,
    SCREEN_RING, SHADE, SHADE_FROM, SIDE_DIM, SIDE_SCALE, SIDES, STEP, TILT,
};
use gpui::{
    AnyElement, App, Context, DevicePixels, Div, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ObjectFit, ParentElement, Pixels, Render, ScrollEvent, Styled, StyledImage, Window, black, div, img,
    linear_color_stop, linear_gradient, point, px, size,
};
use nova_look::motion::Eased;
use nova_look::tokens::{
    BAR_GAP, BAR_HEIGHT, CHIP_PAD, DURATION_FADE, FONT_MONO, FONT_SIZE_HEADING, FONT_SIZE_SMALL, FONT_SIZE_TITLE, GAP,
    ICON_GAP, ICON_SIZE, PANEL_PAD, RADIUS_PANEL, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{Glyph, Icon, Theme};
use nova_stage::Stage;
use std::time::Instant;

/// One card as it is drawn this frame.
struct Placed {
    index: usize,
    away: f32,
    opacity: f32,
}

impl Render for Carousel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.follow_open(Stage::is_open(NAME, self.screen.id, cx), window, cx);
        let now = Instant::now();
        let picking = self.deck.choosing_for.as_deref() == Some(self.screen.name.as_ref());
        self.picking_here.set_target(if picking { 1.0 } else { 0.0 }, now);

        let pixels = self.pixels(window);
        let preview_path = self.deck.preview_path();
        self.preview.show(&preview_path, pixels, cx);
        let preview = self.preview.element(now, cx);
        let deck = self.deck_element(window, now, cx);
        let label = self.label(window, cx);
        let note = self.note(cx);
        self.release(Some(window), cx);
        if self.moving(now) {
            window.request_animation_frame();
        }

        let opening = self.opening.value(now);
        let rise = ENTER_RISE * (1.0 - opening);
        let shade = linear_gradient(
            180.0,
            linear_color_stop(Hsla::transparent_black(), SHADE_FROM),
            linear_color_stop(black().opacity(SHADE), 1.0),
        );
        div()
            .id("wallpaper-carousel")
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .on_scroll(cx.listener(|carousel, event: &ScrollEvent, _, cx| carousel.scrolled(event, cx)))
            // A click beside the deck puts the old image back.
            .on_mouse_down(MouseButton::Left, cx.listener(|carousel, _: &MouseDownEvent, _, cx| carousel.close(cx)))
            .child(preview)
            .child(div().absolute().inset_0().bg(shade))
            .children(picking_mark(self.picking_here.value(now), cx))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(rise)
                    .size_full()
                    .opacity(self.deck_shown.value(now))
                    .children(deck)
                    .children(label)
                    .children(note),
            )
    }
}

/// Stops a click from reaching the background, which closes.
fn swallow_clicks(element: Div) -> Div {
    element.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// The mark on the screen the deck picks for alone, at `opacity`: a ring
/// round the screen's edge and a tag under the bar. None at 0.
pub fn picking_mark(opacity: f32, cx: &App) -> Option<AnyElement> {
    if opacity <= 0.0 {
        return None;
    }
    let theme = Theme::of(cx);
    let tag = div()
        .h(CHIP_HEIGHT + BAR_GAP * 2.0)
        .px(LABEL_PAD)
        .rounded_full()
        .bg(theme.surface())
        .flex()
        .items_center()
        .gap(ICON_GAP)
        .child(div().size(LEGEND_DOT).rounded_full().bg(theme.primary))
        .child(div().text_size(FONT_SIZE_SMALL).font_weight(WEIGHT_LABEL).child("Picking for this screen"));
    let mark = div()
        .absolute()
        .inset_0()
        .opacity(opacity)
        .child(div().absolute().inset_0().border(SCREEN_RING).border_color(theme.fg))
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(BAR_GAP + BAR_HEIGHT + LABEL_GAP)
                .flex()
                .justify_center()
                .child(tag),
        );
    Some(mark.into_any_element())
}

impl Carousel {
    fn card_size(&self) -> (Pixels, Pixels) {
        let screen = self.screen.size;
        let width = (screen.width * CARD_SHARE).round();
        let height = (width * (f32::from(screen.height) / f32::from(screen.width).max(1.0))).round();
        (width, height)
    }

    fn deck_top(&self) -> Pixels {
        ((self.screen.size.height - self.card_size().1) / 2.0).round()
    }

    /// The cards in view, furthest first, each with how much of it shows.
    /// A card coming round to rest halfway (the other image of two) fades
    /// in there rather than popping up.
    fn placed(&mut self, now: Instant) -> Vec<Placed> {
        let count = self.deck.count();
        let position = self.position.value(now);
        let mut placed = Vec::new();
        let mut seen_now = Vec::new();
        for index in 0..count {
            let away = offset(index, position, count);
            if away.abs() > SIDES as f32 + 1.0 {
                continue;
            }
            let seen = shown(away, count, SIDES);
            let before = self.last_seen.iter().find(|(each, _)| *each == index).map_or(0.0, |(_, seen)| *seen);
            if seen - before > 0.5 && !self.last_seen.is_empty() {
                let mut fade = Eased::lasting(before, DURATION_FADE, now);
                fade.set_target(seen, now);
                self.arrivals.retain(|(each, _)| *each != index);
                self.arrivals.push((index, fade));
            }
            let arriving = self.arrivals.iter().find(|(each, fade)| *each == index && !fade.settled(now));
            let opacity = arriving.map_or(seen, |(_, fade)| fade.value(now).min(seen));
            seen_now.push((index, seen));
            if opacity > 0.0 {
                placed.push(Placed { index, away, opacity });
            }
        }
        self.arrivals.retain(|(_, fade)| !fade.settled(now));
        self.last_seen = seen_now;
        placed.sort_by(|a, b| b.away.abs().total_cmp(&a.away.abs()));
        placed
    }

    fn deck_element(&mut self, window: &Window, now: Instant, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (width, height) = self.card_size();
        let top = self.deck_top();
        let spread = ENTER_SPREAD + (1.0 - ENTER_SPREAD) * self.opening.value(now);
        let pixels = device_size(width, height, window);
        let picked = self.deck.picked();
        let middle = self.screen.size.width / 2.0;
        let placed = self.placed(now);
        let deck_shown = self.deck_shown.value(now);
        // Where each card is, the furthest first.
        let turned: Vec<(Tilt, (f32, f32), f32)> = placed
            .iter()
            .map(|card| {
                let near = card.away.abs().min(1.0);
                let far = (card.away.abs() - 1.0).max(0.0);
                let across = card.away.signum() * (near * NEIGHBOUR + far * STEP) * f32::from(width) * spread;
                let sized = 1.0 - near * (1.0 - SIDE_SCALE) - far * DEPTH_SCALE;
                let turn = card.away.clamp(-1.0, 1.0) * TILT;
                let tilt = Tilt::new(f32::from(width) * sized, f32::from(height) * sized, turn);
                (tilt, (f32::from(middle) + across, f32::from(top + height / 2.0)), sized)
            })
            .collect();
        let nearer: Vec<(Tilt, (f32, f32))> = turned.iter().map(|(tilt, at, _)| (*tilt, *at)).collect();
        placed
            .into_iter()
            .enumerate()
            .map(|(order, card)| {
                let path = self.deck.images[card.index].clone();
                let near = card.away.abs().min(1.0);
                let (tilt, at, sized) = turned[order];
                // A chip is drawn while at least half of today's turned one
                // would be in view. Hidden under a nearer card, the flat
                // one is wider and would stick out.
                let marks: Vec<String> = self.deck.marks(&path).into_iter().map(str::to_owned).collect();
                let chips = ChipPlaces::of(&tilt, sized, &marks, window);
                let in_view = |[from, to]: [(f32, f32); 2]| {
                    let half = ((from.0 + to.0) / 2.0, from.1);
                    !tilt.hidden_under(at, half, half, &nearer[order + 1..])
                };
                let wanted = Wanted::new(&path, pixels);
                let look = CardLook {
                    picture: self.cards.update(cx, |pictures, cx| pictures.get(&wanted, cx)),
                    picked: Some(card.index) == picked,
                    now: self.deck.is_now(&path) && in_view(chips.now),
                    marks: if in_view(chips.marks) { marks } else { Vec::new() },
                    dim: near * SIDE_DIM,
                    opacity: card.opacity * deck_shown,
                    sized,
                };
                let by = card.away.round() as i64;
                let clicked = cx.listener(
                    move |carousel, _: &MouseDownEvent, _, cx| {
                        if by == 0 { carousel.choose(cx) } else { carousel.step(by, cx) }
                    },
                );
                draw_card(look, tilt, point(px(at.0), px(at.1)), clicked, cx).opacity(card.opacity).into_any_element()
            })
            .collect()
    }

    /// The picked image's name, where it is in the folder, and where it'd
    /// go; or why it couldn't.
    fn label(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.deck.count() == 0 {
            return None;
        }
        let theme = Theme::of(cx);
        let (fg, dim, error, surface) = (theme.fg, theme.fg_dim(), theme.error, theme.surface());
        let (card_width, card_height) = self.card_size();
        let several = self.deck.screens.len() > 1;
        let (name, color) = if self.problem.is_empty() {
            (file_name(self.deck.picked_path()).to_owned(), fg)
        } else {
            (format!("Couldn't set it: {}", self.problem), error)
        };
        let counted = format!("{} / {}", self.deck.picked().map_or(0, |index| index + 1), self.deck.count());
        let name = div()
            .max_w(card_width)
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis_middle()
            .text_size(FONT_SIZE_HEADING)
            .font_weight(WEIGHT_LABEL)
            .text_color(color)
            .child(name);
        let counted = div().font_family(FONT_MONO).text_size(FONT_SIZE_SMALL).text_color(dim).child(counted);
        let picker = several.then(|| self.picker(window, cx));
        let tab = several.then(|| div().font_family(FONT_MONO).text_size(FONT_SIZE_SMALL).text_color(dim).child("Tab"));
        let label = div()
            .h(if several { PICKER_HEIGHT } else { LABEL_HEIGHT })
            .px(LABEL_PAD)
            .rounded_full()
            .bg(surface)
            .flex()
            .items_center()
            .gap(LABEL_PAD)
            .child(name)
            .child(counted)
            .children(picker)
            .children(tab);
        let row = div()
            .absolute()
            .left_0()
            .right_0()
            .top(self.deck_top() + card_height + LABEL_GAP)
            .flex()
            .justify_center()
            .child(swallow_clicks(label));
        Some(row.into_any_element())
    }

    /// Which screen the deck picks for: every one, or one of them drawn to
    /// scale with its pick.
    fn picker(&mut self, window: &Window, cx: &mut Context<Self>) -> Div {
        let theme = Theme::of(cx);
        let (fg, dim, rim, quiet) = (theme.fg, theme.fg_dim(), theme.glass_rim(), theme.selected_quiet());
        let every = self.deck.choosing_for.is_none();
        let ink = if every { fg } else { dim };
        let chip = div();
        let chip = if every { chip.bg(quiet) } else { chip.border_1().border_color(rim) };
        let all = chip
            .h(CHIP_HEIGHT)
            .px(CHIP_PAD)
            .rounded_full()
            .flex()
            .items_center()
            .gap(ICON_GAP)
            .child(Icon::new(Glyph::MONITORS).size(ICON_SIZE).color(ink))
            .child(div().text_size(FONT_SIZE_SMALL).text_color(ink).child("All"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|carousel, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    carousel.choose_for(None, cx);
                }),
            );
        let sizes = self.screen_sizes(cx);
        let widest = sizes.iter().map(|(_, screen)| f32::from(screen.width)).fold(1.0, f32::max);
        let minis: Vec<AnyElement> = sizes
            .into_iter()
            .map(|(name, screen)| {
                let width = (MINI_WIDTH * (f32::from(screen.width) / widest)).round();
                let height = (width * (f32::from(screen.height) / f32::from(screen.width).max(1.0))).round();
                self.mini(name, size(width, height), window, cx)
            })
            .collect();
        div().flex().items_center().gap(MINI_GAP).child(all).children(minis)
    }

    /// One screen in the picker: its pick, small, shaped like the screen,
    /// with its name under it; ringed while the deck picks for it.
    fn mini(&mut self, name: String, drawn: gpui::Size<Pixels>, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let (fg, dim, rim, loading) = (theme.fg, theme.fg_dim(), theme.glass_rim(), theme.fg.opacity(LOADING));
        let (width, height) = (drawn.width, drawn.height);
        let chosen = self.deck.choosing_for.as_deref() == Some(name.as_str());
        let pick = self.deck.picks.get(&name).cloned().unwrap_or_default();
        let pixels = device_size(width, height, window);
        let picture = (!pick.is_empty())
            .then(|| self.minis.update(cx, |pictures, cx| pictures.get(&Wanted::new(&pick, pixels), cx)))
            .flatten();
        // Chosen, a ring round it; else a faint edge on it.
        let (ring, out) = if chosen { (MINI_RING, MINI_RING) } else { (px(1.0), px(0.0)) };
        let frame = div()
            .absolute()
            .left(-out)
            .top(-out)
            .w(width + out * 2.0)
            .h(height + out * 2.0)
            .rounded(MINI_RADIUS + out)
            .border(ring)
            .border_color(if chosen { fg } else { rim });
        let caption = div()
            .absolute()
            .top(height + MINI_RING + px(1.0))
            .left(-MINI_WIDTH)
            .right(-MINI_WIDTH)
            .flex()
            .justify_center()
            .font_family(FONT_MONO)
            .text_size(FONT_SIZE_SMALL)
            .text_color(if chosen { fg } else { dim })
            .child(name.clone());
        div()
            .relative()
            .w(width)
            .h(height)
            .opacity(if chosen { 1.0 } else { MINI_IDLE })
            .child(div().absolute().inset_0().rounded(MINI_RADIUS).bg(loading))
            .children(picture.map(|picture| {
                img(picture).absolute().inset_0().size_full().rounded(MINI_RADIUS).object_fit(ObjectFit::Fill)
            }))
            .child(frame)
            .child(caption)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |carousel, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    carousel.choose_for(Some(name.clone()), cx);
                }),
            )
            .into_any_element()
    }

    /// Every screen the deck picks for, in its order, with its size.
    fn screen_sizes(&self, cx: &App) -> Vec<(String, gpui::Size<Pixels>)> {
        let screens = Stage::screens(cx);
        self.deck
            .screens
            .iter()
            .filter_map(|name| {
                let screens = screens.as_ref()?.read(cx);
                let screen = screens.list().iter().find(|screen| screen.name.as_ref() == name)?;
                Some((name.clone(), screen.size))
            })
            .collect()
    }

    /// No images: where to put some, or why the folder couldn't be read.
    fn note(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.deck.count() > 0 || self.loading {
            return None;
        }
        let theme = Theme::of(cx);
        let title = if self.problem.is_empty() {
            format!("No wallpapers in {}", self.deck.folder)
        } else {
            "Can't show your wallpapers".to_owned()
        };
        let why = (!self.problem.is_empty())
            .then(|| div().w_full().text_center().text_color(theme.fg_dim()).child(self.problem.clone()));
        let note = div()
            .w(NOTE_WIDTH)
            .p(PANEL_PAD)
            .rounded(RADIUS_PANEL)
            .bg(theme.surface())
            .flex()
            .flex_col()
            .gap(GAP)
            .child(div().w_full().text_center().text_size(FONT_SIZE_TITLE).font_weight(WEIGHT_TITLE).child(title))
            .children(why);
        let centered = div().absolute().inset_0().flex().items_center().justify_center().child(swallow_clicks(note));
        Some(centered.into_any_element())
    }
}

/// `width` by `height` on `window`'s screen, in its pixels.
pub fn device_size(width: Pixels, height: Pixels, window: &Window) -> gpui::Size<DevicePixels> {
    let scale = window.scale_factor();
    let side = |length: Pixels| DevicePixels((f32::from(length) * scale).round() as i32);
    size(side(width), side(height))
}
