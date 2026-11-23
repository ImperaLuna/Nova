//! `tray.card`: every tray app in quick settings, hidden ones too and
//! faded, in a grid a few rows high that scrolls. A left click on one with
//! a menu shows that menu over the drawer, below its icon; on any other it
//! runs the app and closes quick settings. A right click shows the menu, a
//! middle click is the app's secondary action. `TrayCard.qml` and
//! `TrayCardSlot.qml` in today's shell.

use crate::menu::TrayMenu;
use crate::model::{self, Entry};
use crate::strip::attention_dot;
use crate::tokens::{
    MENU_GAP, TRAY_GRID_CELL, TRAY_GRID_COLUMNS, TRAY_GRID_GAP, TRAY_GRID_ICON_SIZE, TRAY_GRID_MAX_ROWS,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, AnyElement, AppContext, Bounds, Context, Entity, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, Pixels, Point, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, anchored, canvas, deferred, div, point,
};
use nova_look::tokens::{DIM_OPACITY, DIVIDER_WIDTH, GAP, MENU_PAD, PAD, RADIUS_CARD, RADIUS_CONTROL};
use nova_look::{AppIcon, CardTitle, Theme};
use nova_stage::{Screen, Shown, Stage};
use nova_tray_api::{Command, Event, TOPIC, Tray};
use nova_ui_client::{Daemon, Live};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Quick settings' drawer, by its name, which running an app closes.
const QUICK_SETTINGS: &str = "quick-settings";

pub struct Card {
    screen: Screen,
    shown: Shown,
    tray: Entity<Live<Tray>>,
    apps: Vec<Entry>,
    /// The menu drawn over the drawer, this card's own.
    menu: Entity<TrayMenu>,
    /// Where it shows: below the middle of the icon clicked, in the
    /// window.
    menu_at: Point<Pixels>,
    /// Each icon's middle in the window, as it was last drawn.
    middles: Rc<RefCell<HashMap<String, Point<Pixels>>>>,
    _followed: Vec<Subscription>,
}

impl Card {
    pub fn new(screen: &Screen, shown: Shown, cx: &mut Context<Self>) -> Self {
        let tray = Live::follow(TOPIC, cx, |Event::Tray(tray)| Some(tray));
        let menu = cx.new(|_| TrayMenu::in_drawer(screen));
        let mut followed =
            vec![cx.observe(&tray, |card, _, cx| card.read(cx)), cx.observe(&menu, |_, _, cx| cx.notify())];
        // Quick settings closed: the menu goes with it.
        followed.extend(Stage::observe_panels(cx, |card: &mut Self, cx| {
            if !Stage::is_open(QUICK_SETTINGS, card.screen.id, cx) {
                card.menu.update(cx, |menu, cx| menu.dismiss(cx));
            }
        }));
        let mut card = Self {
            screen: screen.clone(),
            shown,
            tray,
            apps: Vec::new(),
            menu,
            menu_at: Point::default(),
            middles: Rc::default(),
            _followed: followed,
        };
        card.read(cx);
        card
    }

    /// Takes novad's latest tray: every app, in the order they came.
    fn read(&mut self, cx: &mut Context<Self>) {
        self.apps = self.tray.read(cx).get().map(model::entries).unwrap_or_default();
        // The drawer redraws when this shows or hides the card.
        self.shown.set(!self.apps.is_empty(), cx);
        cx.notify();
    }

    fn clicked(&mut self, app: &Entry, button: MouseButton, cx: &mut Context<Self>) {
        match button {
            MouseButton::Left if !app.opens_menu => {
                Daemon::send(Command::ActivateTrayItem { address: app.address.clone() }, cx);
                let screen = self.screen.id;
                cx.defer(move |cx| Stage::close(QUICK_SETTINGS, screen, cx));
            }
            MouseButton::Left | MouseButton::Right => {
                self.menu_at = self.middles.borrow().get(&app.key).copied().unwrap_or_default();
                let app = app.clone();
                self.menu.update(cx, |menu, cx| menu.show(app, self.menu_at.x, cx));
            }
            MouseButton::Middle => {
                Daemon::send(Command::SecondaryActivateTrayItem { address: app.address.clone() }, cx);
            }
            _ => {}
        }
    }

    fn icon(&self, app: &Entry, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let hover = theme.hover();
        let (middles, key) = (self.middles.clone(), app.key.clone());
        let record = canvas(
            move |bounds: Bounds<Pixels>, _, _| {
                middles.borrow_mut().insert(key.clone(), bounds.center());
            },
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0();
        let picture = div()
            .when(app.hidden, |picture| picture.opacity(DIM_OPACITY))
            .child(AppIcon::new(app.key.clone(), app.name.clone()).icon(app.icon.clone()).size(TRAY_GRID_ICON_SIZE));
        let mut cell = div()
            .id(SharedString::from(format!("tray-card-{}", app.key)))
            .relative()
            .h(TRAY_GRID_CELL)
            .rounded(RADIUS_CONTROL)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .child(record)
            .child(picture)
            .children(app.attention.then(|| attention_dot(1.0, cx)));
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            let app = app.clone();
            cell = cell.on_mouse_down(
                button,
                cx.listener(move |card, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    card.clicked(&app, button, cx);
                }),
            );
        }
        cell.into_any_element()
    }

    /// The menu below the icon clicked, over everything else in the
    /// drawer; a click beside it closes it.
    fn menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if !self.menu.read(cx).is_showing() {
            return None;
        }
        let theme = Theme::of(cx);
        let at = self.menu_at + point(Pixels::ZERO, TRAY_GRID_CELL / 2.0 + MENU_GAP);
        let width = self.menu.read(cx).size().width;
        let menu = self.menu.clone();
        let boxed = div()
            .id("tray-card-menu")
            .occlude()
            .rounded(RADIUS_CARD)
            .bg(theme.surface())
            .border(DIVIDER_WIDTH)
            .border_color(theme.outline())
            .on_mouse_down_out(move |_, _, cx| menu.update(cx, |menu, cx| menu.dismiss(cx)))
            .child(self.menu.clone());
        Some(deferred(
            anchored()
                .anchor(Anchor::TopLeft)
                .position(at - point(width / 2.0, Pixels::ZERO))
                .snap_to_window_with_margin(PAD)
                .child(boxed),
        ))
    }
}

impl Render for Card {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.apps.len();
        let apps = self.apps.clone();
        let cells: Vec<AnyElement> = apps.iter().map(|app| self.icon(app, cx)).collect();
        let theme = Theme::of(cx);
        let mut rows = Vec::new();
        let mut cells = cells.into_iter().peekable();
        while cells.peek().is_some() {
            let mut row: Vec<AnyElement> = cells
                .by_ref()
                .take(TRAY_GRID_COLUMNS)
                .map(|cell| div().flex_1().min_w_0().child(cell).into_any_element())
                .collect();
            // A short last row keeps the columns' width.
            row.resize_with(TRAY_GRID_COLUMNS, || div().flex_1().into_any_element());
            rows.push(div().flex().gap(TRAY_GRID_GAP).children(row));
        }
        let most = TRAY_GRID_CELL * TRAY_GRID_MAX_ROWS as f32 + TRAY_GRID_GAP * (TRAY_GRID_MAX_ROWS - 1) as f32;
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(GAP)
            .child(
                div().flex().justify_between().child(CardTitle::new("Tray")).child(CardTitle::new(apps_count(count))),
            )
            .child(
                div().p(MENU_PAD).rounded(RADIUS_CARD).bg(theme.surface_raised()).child(
                    div()
                        .id("tray-grid")
                        .max_h(most)
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap(TRAY_GRID_GAP)
                        .children(rows),
                ),
            )
            .children(self.menu(cx))
    }
}

/// "1 app", "7 apps".
fn apps_count(count: usize) -> String {
    match count {
        1 => "1 app".to_owned(),
        count => format!("{count} apps"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_says_app_or_apps() {
        assert_eq!(apps_count(1), "1 app");
        assert_eq!(apps_count(7), "7 apps");
    }
}
