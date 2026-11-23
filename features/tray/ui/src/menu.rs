//! A tray app's menu under its icon (panel `tray-menu`), ported from
//! `ui/tray/TrayMenu.qml` and `TrayMenuList.qml`: the app's icon and name,
//! the entries its DBusMenu lists one level at a time (separators,
//! checkboxes, submenus with a way back), and Nova's own "Hide from tray"
//! (or "Show in tray"). It drops into place beside the open tray, closes
//! with it, and once an entry ran; a checkbox flips in place.
//!
//! novad reads the menu and runs its entries (`tray_menu`,
//! `run_tray_menu_entry`): the stage owns the glass, the drop in, Esc and
//! a click outside.

use crate::model::{self, Entry};
use crate::strip::TRAY;
use crate::tokens::{
    MENU_CHECK_ICON_SIZE, MENU_CHECK_RADIUS, MENU_CHECK_SIZE, MENU_GAP, MENU_ROW_HEIGHT, MENU_ROW_RADIUS,
    MENU_SEPARATOR_INSET, MENU_WIDTH, TRAY_ICON_SIZE,
};
use gpui::{
    AnyElement, App, AppContext, Context, Entity, Global, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Pixels, Render, SharedString, Size, StatefulInteractiveElement, Styled, Task, Window, div, size,
};
use nova_look::tokens::{
    BAR_GAP, BAR_HEIGHT, BAR_MARGIN, DIVIDER_WIDTH, ICON_GAP, ICON_SIZE_SMALL, MENU_PAD, PILL_BUTTON_SIZE, ROW_INSET,
    STROKE_THIN, WEIGHT_TITLE,
};
use nova_look::{AppIcon, Glyph, Icon, Theme};
use nova_stage::{Panel, Screen, Stage};
use nova_tray_api::{Command, Toggle, TrayMenuEntry, TrayMenuLevel};
use nova_ui_client::Daemon;
use std::collections::HashMap;

/// The panel's name, as today's shell calls it.
pub const MENU: &str = "tray-menu";
/// Quick settings' drawer, by its name, which a menu drawn over it closes.
const QUICK_SETTINGS: &str = "quick-settings";

/// How far down the screen the menu's top is: below the bar.
fn top() -> Pixels {
    BAR_GAP + BAR_HEIGHT + MENU_GAP
}

/// Each screen's menu, shared by the strip that opens it and the panel
/// that draws it.
#[derive(Default)]
struct Menus(HashMap<gpui::DisplayId, Entity<TrayMenu>>);

impl Global for Menus {}

/// `screen`'s menu, made the first time either side asks.
pub fn of(screen: &Screen, cx: &mut App) -> Entity<TrayMenu> {
    if let Some(menu) = cx.default_global::<Menus>().0.get(&screen.id) {
        return menu.clone();
    }
    let menu = cx.new(|_| TrayMenu::new(screen));
    cx.global_mut::<Menus>().0.insert(screen.id, menu.clone());
    menu
}

/// The menu's panel on `screen`: it drops into place where the strip put
/// it, beside the open tray.
pub fn panel(screen: &Screen, cx: &mut App) -> Panel {
    let menu = of(screen, cx);
    let size = menu.read(cx).size();
    Panel::new(MENU, size, menu).exclusive(false).top(top()).radius(nova_look::tokens::RADIUS_CARD)
}

pub struct TrayMenu {
    screen: Screen,
    /// Drawn over quick settings' Tray card instead of as a panel below
    /// the bar ([`TrayMenu::in_drawer`]).
    in_drawer: bool,
    /// The app whose menu it shows.
    entry: Option<Entry>,
    /// Submenus opened, the innermost last.
    stack: Vec<i32>,
    level: TrayMenuLevel,
    /// The level novad hasn't answered yet; a newer one drops it.
    asking: Option<Task<()>>,
    running: Option<Task<()>>,
}

impl TrayMenu {
    fn new(screen: &Screen) -> Self {
        Self {
            screen: screen.clone(),
            in_drawer: false,
            entry: None,
            stack: Vec::new(),
            level: TrayMenuLevel::default(),
            asking: None,
            running: None,
        }
    }

    /// A menu the Tray card in quick settings draws over the drawer
    /// itself: it is no panel, an entry that ran closes quick settings
    /// (hiding or showing the app leaves it open, to see the change), and
    /// the card closes it on a click beside it ([`TrayMenu::dismiss`]).
    pub fn in_drawer(screen: &Screen) -> Self {
        Self { in_drawer: true, ..Self::new(screen) }
    }

    /// Whether it shows an app's menu now.
    pub fn is_showing(&self) -> bool {
        self.entry.is_some()
    }

    /// Stops showing it (a click beside a menu in the drawer).
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.entry.take().is_some() {
            cx.notify();
        }
    }

    /// The tallest it may be: down to the bar's margin above the screen's
    /// bottom, or the drawer's height.
    fn max_height(&self) -> Pixels {
        match self.in_drawer {
            true => self.screen.size.height - BAR_GAP - BAR_MARGIN * 2.0,
            false => self.screen.size.height - top() - BAR_MARGIN,
        }
    }

    pub fn size(&self) -> Size<Pixels> {
        let (_, height) = model::menu_heights(&self.level.entries, !self.stack.is_empty(), self.max_height());
        size(MENU_WIDTH, height)
    }

    /// Shows `entry`'s menu, right-aligned a little past its icon, whose
    /// middle is `icon_x` across the screen.
    pub fn show(&mut self, entry: Entry, icon_x: Pixels, cx: &mut Context<Self>) {
        let screen = self.screen.id;
        let left = icon_x + PILL_BUTTON_SIZE / 2.0 + MENU_GAP - MENU_WIDTH;
        self.entry = Some(entry);
        self.stack.clear();
        self.level = TrayMenuLevel::default();
        self.read(cx);
        if !self.in_drawer {
            self.resize(cx);
            cx.defer(move |cx| {
                Stage::place(MENU, screen, left, cx);
                Stage::open(MENU, screen, cx);
            });
        }
        cx.notify();
    }

    /// Reads the level shown now.
    fn read(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = &self.entry else { return };
        if !entry.has_menu {
            return;
        }
        let id = self.stack.last().copied().unwrap_or(0);
        let reply = Daemon::call::<TrayMenuLevel>(Command::TrayMenu { address: entry.address.clone(), id }, cx);
        self.asking = Some(cx.spawn(async move |menu, cx| {
            let level = reply.await;
            let _ = menu.update(cx, |menu, cx| {
                match level {
                    Ok(level) => menu.level = level,
                    Err(refusal) => eprintln!("WARN nova-ui: tray: {refusal}"),
                }
                menu.resize(cx);
                cx.notify();
            });
        }));
    }

    fn resize(&self, cx: &mut Context<Self>) {
        if self.in_drawer {
            // Not a panel: its view takes the room it needs.
            return;
        }
        let (screen, size) = (self.screen.id, self.size());
        cx.defer(move |cx| Stage::resize(MENU, screen, size, cx));
    }

    fn open_submenu(&mut self, id: i32, cx: &mut Context<Self>) {
        self.stack.push(id);
        self.level = TrayMenuLevel::default();
        self.read(cx);
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        self.stack.pop();
        self.level = TrayMenuLevel::default();
        self.read(cx);
    }

    /// Runs `picked`: novad notes the app's windows, then clicks it. A
    /// checkbox flips in place, anything else is done.
    fn run(&mut self, picked: &TrayMenuEntry, cx: &mut Context<Self>) {
        let Some(entry) = &self.entry else { return };
        let command =
            Command::RunTrayMenuEntry { address: entry.address.clone(), id: picked.id, revision: self.level.revision };
        let reply = Daemon::call::<()>(command, cx);
        let flips = picked.toggle != Toggle::None;
        self.running = Some(cx.spawn(async move |menu, cx| {
            if let Err(refusal) = reply.await {
                eprintln!("WARN nova-ui: tray: {refusal}");
            }
            if flips {
                let _ = menu.update(cx, |menu, cx| menu.read(cx));
            }
        }));
        if !flips {
            self.done(false, cx);
        }
    }

    fn hide(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = &self.entry else { return };
        Daemon::send(Command::HideTrayApp { id: entry.key.clone(), hidden: !entry.hidden }, cx);
        self.done(true, cx);
    }

    /// Closes the menu and the tray with it; in the drawer, quick settings
    /// too unless `stay` (the app was hidden or shown).
    fn done(&mut self, stay: bool, cx: &mut Context<Self>) {
        let screen = self.screen.id;
        if self.in_drawer {
            self.dismiss(cx);
            if !stay {
                cx.defer(move |cx| Stage::close(QUICK_SETTINGS, screen, cx));
            }
            return;
        }
        cx.defer(move |cx| {
            Stage::close(MENU, screen, cx);
            Stage::close(TRAY, screen, cx);
        });
    }
}

impl Render for TrayMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(entry) = self.entry.clone() else { return div() };
        let fg = Theme::of(cx).fg;
        let in_submenu = !self.stack.is_empty();
        let (list_height, _) = model::menu_heights(&self.level.entries, in_submenu, self.max_height());
        let app = div()
            .h(MENU_ROW_HEIGHT)
            .pl(ROW_INSET)
            .flex()
            .items_center()
            .gap(ICON_GAP)
            .child(AppIcon::new(entry.key.clone(), entry.name.clone()).icon(entry.icon.clone()).size(TRAY_ICON_SIZE))
            .child(div().flex_1().min_w_0().truncate().font_weight(WEIGHT_TITLE).child(entry.name.clone()));
        let back = in_submenu.then(|| {
            row(RowLook { text: "Back".into(), mark: Mark::Icon(Glyph::LEFT), ..RowLook::default() }, cx)
                .on_mouse_down(MouseButton::Left, cx.listener(|menu, _: &MouseDownEvent, _, cx| menu.back(cx)))
        });
        let entries: Vec<AnyElement> = self.level.entries.iter().map(|picked| self.entry_row(picked, cx)).collect();
        let has_entries = !entries.is_empty();
        let list = div().id("entries").h(list_height).overflow_y_scroll().flex().flex_col().children(entries);
        let nova_own = RowLook {
            text: if entry.hidden { "Show in tray" } else { "Hide from tray" }.into(),
            ink: Ink::Quiet,
            ..RowLook::default()
        };
        let hide = row(nova_own, cx)
            .on_mouse_down(MouseButton::Left, cx.listener(|menu, _: &MouseDownEvent, _, cx| menu.hide(cx)));
        div()
            .w(MENU_WIDTH)
            .p(MENU_PAD)
            .flex()
            .flex_col()
            .text_color(fg)
            .child(app)
            .children(back)
            .child(list)
            .children(has_entries.then(|| separator(cx)))
            .child(hide)
    }
}

impl TrayMenu {
    fn entry_row(&self, picked: &TrayMenuEntry, cx: &mut Context<Self>) -> AnyElement {
        if picked.separator {
            return separator(cx).into_any_element();
        }
        let text = model::label(&picked.label);
        let look = RowLook {
            ink: if model::is_quit(&text) { Ink::Quit } else { Ink::Plain },
            text: text.into(),
            mark: if picked.toggle == Toggle::None { Mark::None } else { Mark::Check(picked.checked) },
            opens: picked.submenu,
            enabled: picked.enabled,
        };
        let row = row(look, cx);
        if !picked.enabled {
            return row.into_any_element();
        }
        let picked = picked.clone();
        row.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |menu, _: &MouseDownEvent, _, cx| {
                if picked.submenu {
                    menu.open_submenu(picked.id, cx);
                } else {
                    menu.run(&picked, cx);
                }
            }),
        )
        .into_any_element()
    }
}

/// What sits before a row's text: a checkbox, an icon, or the room either
/// takes, so labels line up.
#[derive(Default, Clone, Copy)]
enum Mark {
    #[default]
    None,
    Check(bool),
    Icon(Glyph),
}

#[derive(Default, Clone, Copy, PartialEq)]
enum Ink {
    #[default]
    Plain,
    /// Ends the app: in the error color.
    Quit,
    /// Nova's own entry, set apart.
    Quiet,
}

struct RowLook {
    text: SharedString,
    mark: Mark,
    ink: Ink,
    /// Has a submenu.
    opens: bool,
    enabled: bool,
}

impl Default for RowLook {
    fn default() -> Self {
        Self { text: SharedString::default(), mark: Mark::None, ink: Ink::Plain, opens: false, enabled: true }
    }
}

fn row(look: RowLook, cx: &App) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let ink = match look.ink {
        Ink::Plain => theme.fg,
        Ink::Quit => theme.error,
        Ink::Quiet => theme.fg_dim(),
    };
    let mark =
        div().size(MENU_CHECK_SIZE).flex_none().rounded(MENU_CHECK_RADIUS).flex().items_center().justify_center();
    let mark = match look.mark {
        Mark::None => mark,
        Mark::Check(true) => {
            mark.bg(theme.primary).child(Icon::new(Glyph::CHECK).size(MENU_CHECK_ICON_SIZE).color(theme.on_accent))
        }
        Mark::Check(false) => mark.border(STROKE_THIN).border_color(theme.fg_dim()),
        Mark::Icon(glyph) => mark.child(Icon::new(glyph).size(ICON_SIZE_SMALL).color(ink)),
    };
    let opens = look.opens.then(|| Icon::new(Glyph::RIGHT).size(ICON_SIZE_SMALL).color(theme.fg_dim()));
    let hover = theme.hover();
    let row = div()
        .id(look.text.clone())
        .h(MENU_ROW_HEIGHT)
        .flex_none()
        .px(ROW_INSET)
        .rounded(MENU_ROW_RADIUS)
        .flex()
        .items_center()
        .gap(ROW_INSET)
        .child(mark)
        .child(div().flex_1().min_w_0().truncate().text_color(ink).child(look.text))
        .children(opens);
    if look.enabled {
        row.cursor_pointer().hover(move |style| style.bg(hover))
    } else {
        row.opacity(nova_look::tokens::DIM_OPACITY)
    }
}

fn separator(cx: &App) -> gpui::Div {
    div()
        .h(model::separator_height())
        .flex_none()
        .px(MENU_SEPARATOR_INSET)
        .flex()
        .items_center()
        .child(div().h(DIVIDER_WIDTH).w_full().bg(Theme::of(cx).outline()))
}
