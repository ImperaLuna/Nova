//! What the tray works out, with no GPUI in it: which apps it shows and in
//! which order, what the strip and the "+N" counts show as it scrolls, how
//! a menu entry reads and how tall a menu is. `ui/tray/tray.js` in today's
//! shell; its tests (`ui/tests/tst_tray.qml`) are mirrored here.

use crate::tokens::{MENU_ROW_HEIGHT, MENU_SEPARATOR_GAP, TRAY_FADE};
use gpui::{Pixels, px};
use nova_look::tokens::MENU_PAD;
use nova_look::tokens::{DIVIDER_WIDTH, PILL_BUTTON_GAP, PILL_BUTTON_SIZE};
use nova_tray_api::{Tray, TrayMenuEntry};

/// Menu entries that end the app: drawn in the error color, as the mockup
/// has them.
const QUIT_WORDS: [&str; 2] = ["quit", "exit"];

/// One app in the tray, as the bar and its menu use it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// novad's address for it, which every command names.
    pub address: String,
    /// What `hide_tray_app` knows it by: its app's id, or its own.
    pub key: String,
    pub name: String,
    /// The picture to draw: its app's icon file, else its own.
    pub icon: Option<String>,
    pub attention: bool,
    pub hidden: bool,
    /// It has a DBusMenu to read.
    pub has_menu: bool,
    /// A click shows its menu rather than activating it.
    pub opens_menu: bool,
}

/// The tray's apps in the order they registered.
pub fn entries(tray: &Tray) -> Vec<Entry> {
    tray.items
        .iter()
        .map(|item| {
            let app = item.app.as_ref();
            let key = app.map_or_else(|| item.id.clone(), |app| app.id.clone());
            let named = |text: &str| (!text.is_empty()).then(|| text.to_owned());
            let name =
                app.and_then(|app| named(&app.name)).or_else(|| named(&item.title)).unwrap_or_else(|| item.id.clone());
            let app_icon = app.and_then(|app| app.icon.clone()).filter(|icon| icon.starts_with('/'));
            Entry {
                address: item.address.clone(),
                hidden: tray.hidden.contains(&key),
                key,
                name,
                icon: app_icon.or_else(|| item.icon.clone()),
                attention: item.attention,
                has_menu: item.has_menu,
                opens_menu: (item.is_menu || item.menu_only) && item.has_menu,
            }
        })
        .collect()
}

/// What the bar's strip shows: the apps not hidden, those asking for
/// attention first, otherwise in order.
pub fn shown(all: &[Entry]) -> Vec<Entry> {
    let visible = all.iter().filter(|entry| !entry.hidden);
    let (asking, rest): (Vec<&Entry>, Vec<&Entry>) = visible.partition(|entry| entry.attention);
    asking.into_iter().chain(rest).cloned().collect()
}

/// Whether any app the bar shows asks for attention.
pub fn any_attention(all: &[Entry]) -> bool {
    all.iter().any(|entry| entry.attention && !entry.hidden)
}

/// Icons out of sight on each side of a strip showing `visible` of
/// `total`, scrolled `scrolled` icon widths in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfSight {
    pub left: usize,
    pub right: usize,
}

pub fn out_of_sight(total: usize, visible: usize, scrolled: f32) -> OutOfSight {
    let most = total.saturating_sub(visible);
    let left = (scrolled.round().max(0.0) as usize).min(most);
    OutOfSight { left, right: most - left }
}

/// The distance from one icon of the strip to the next.
pub fn step() -> Pixels {
    PILL_BUTTON_SIZE + PILL_BUTTON_GAP
}

/// How wide `icons` icons side by side are.
pub fn row_width(icons: usize) -> Pixels {
    if icons == 0 {
        return px(0.0);
    }
    PILL_BUTTON_SIZE * icons as f32 + PILL_BUTTON_GAP * (icons - 1) as f32
}

/// The strip's scroll after `notches` wheel notches (down is negative),
/// counted from `from`: whole icons, kept inside the row.
pub fn scrolled(from: Pixels, notches: i32, total: usize, visible: usize) -> Pixels {
    let step = step();
    let furthest = (row_width(total) - row_width(total.min(visible))).max(px(0.0));
    let target = from - step * notches as f32;
    (step * (target / step).round()).clamp(px(0.0), furthest)
}

/// How much of an icon `in_view` from the strip's left edge shows: it
/// fades out over the strip's ends where more are out of sight.
pub fn edge_fade(in_view: Pixels, strip_width: Pixels, fades_left: bool, fades_right: bool) -> f32 {
    let half = PILL_BUTTON_SIZE / 2.0;
    let left = if fades_left { (in_view + half) / TRAY_FADE } else { 1.0 };
    let right = if fades_right { (strip_width - in_view - half) / TRAY_FADE } else { 1.0 };
    left.min(right).clamp(0.0, 1.0)
}

/// Whether a menu entry with `text` ends the app.
pub fn is_quit(text: &str) -> bool {
    let word = text.split_whitespace().next().unwrap_or_default().to_lowercase();
    QUIT_WORDS.contains(&word.as_str())
}

/// A menu entry's text as shown: without the `_` mnemonic markers DBusMenu
/// labels carry (`__` is a literal underscore). Electron apps written for
/// Windows menus send `&&` for a literal `&`.
pub fn label(text: &str) -> String {
    text.replace("__", "\0").replace('_', "").replace('\0', "_").replace("&&", "&")
}

/// A separator's height: a line with a gap above and below.
pub fn separator_height() -> Pixels {
    MENU_SEPARATOR_GAP * 2.0 + DIVIDER_WIDTH
}

/// How tall the app's own entries are, all of them.
pub fn entries_height(entries: &[TrayMenuEntry]) -> Pixels {
    entries
        .iter()
        .map(|entry| if entry.separator { separator_height() } else { MENU_ROW_HEIGHT })
        .fold(px(0.0), |sum, height| sum + height)
}

/// Everything of a menu but the app's entries: the padding, the app's row,
/// Back in a submenu, the separator and Nova's own entry.
pub fn chrome_height(in_submenu: bool, has_entries: bool) -> Pixels {
    let rows = if in_submenu { 3.0 } else { 2.0 };
    MENU_PAD * 2.0 + MENU_ROW_HEIGHT * rows + if has_entries { separator_height() } else { px(0.0) }
}

/// How tall the list of entries is, scrolling past `max_height` for the
/// whole menu, and the menu's height with it.
pub fn menu_heights(entries: &[TrayMenuEntry], in_submenu: bool, max_height: Pixels) -> (Pixels, Pixels) {
    let chrome = chrome_height(in_submenu, !entries.is_empty());
    let list = entries_height(entries).min((max_height - chrome).max(MENU_ROW_HEIGHT));
    (list, chrome + list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_app_info::App;
    use nova_tray_api::{Toggle, TrayItem};

    fn item(address: &str, id: &str, title: &str, app: Option<(&str, &str, Option<&str>)>) -> TrayItem {
        TrayItem {
            address: address.into(),
            id: id.into(),
            title: title.into(),
            app: app.map(|(id, name, icon)| App {
                id: id.into(),
                name: name.into(),
                description: None,
                icon: icon.map(Into::into),
            }),
            menu_only: false,
            icon: None,
            attention: false,
            has_menu: true,
            is_menu: false,
        }
    }

    /// The tray as tst_tray.qml has it: Discord (Electron, no title),
    /// Steam (libayatana, hidden) and Claude asking for attention.
    fn tray() -> Tray {
        let discord = item(
            ":1.421/StatusNotifierItem",
            "discord_status_icon_1",
            "",
            Some(("discord.desktop", "Discord", Some("/icons/discord.png"))),
        );
        let steam = TrayItem {
            menu_only: true,
            icon: Some("/run/nova/tray/1-ab.png".into()),
            ..item(":1.607/org/ayatana/NotificationItem/steam", "steam", "Steam", None)
        };
        let claude = TrayItem {
            attention: true,
            ..item(
                ":1.88/StatusNotifierItem",
                "chrome_status_icon_1",
                "",
                Some(("com.anthropic.Claude.desktop", "Claude", None)),
            )
        };
        Tray { items: vec![discord, steam, claude], hidden: vec!["steam".into()] }
    }

    #[test]
    fn items_are_matched_to_their_apps() {
        let all = entries(&tray());
        let keys: Vec<&str> = all.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(keys, ["discord.desktop", "steam", "com.anthropic.Claude.desktop"]);
        let names: Vec<&str> = all.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["Discord", "Steam", "Claude"], "Electron's empty titles get the app's name");
        assert_eq!(all[0].icon.as_deref(), Some("/icons/discord.png"));
        assert_eq!(all[1].icon.as_deref(), Some("/run/nova/tray/1-ab.png"), "no app: the item's own picture");
        assert_eq!(all.iter().map(|entry| entry.hidden).collect::<Vec<_>>(), [false, true, false]);
        assert_eq!(
            all.iter().map(|entry| entry.opens_menu).collect::<Vec<_>>(),
            [false, true, false],
            "no Activate: its menu"
        );
    }

    #[test]
    fn the_bar_shows_attention_first_and_hides_hidden_apps() {
        let all = entries(&tray());
        let keys: Vec<String> = shown(&all).into_iter().map(|entry| entry.key).collect();
        assert_eq!(keys, ["com.anthropic.Claude.desktop", "discord.desktop"]);
        assert!(any_attention(&all));
    }

    #[test]
    fn an_item_without_an_app_or_title_goes_by_its_id() {
        let all = entries(&Tray { items: vec![item(":1.5/Item", "xembed_1", "", None)], hidden: vec![] });
        assert_eq!((all[0].key.as_str(), all[0].name.as_str()), ("xembed_1", "xembed_1"));
        assert_eq!(all[0].icon, None);
    }

    #[test]
    fn menu_text() {
        assert_eq!(label("_Open Discord"), "Open Discord");
        assert_eq!(label("snake__case"), "snake_case");
        assert_eq!(label("Disable Notifications && Audio"), "Disable Notifications & Audio");
        assert!(is_quit("Quit Discord"));
        assert!(is_quit("Exit Steam"));
        assert!(!is_quit("Quick Entry"));
        assert!(!is_quit(""));
    }

    #[test]
    fn more_counts_what_is_out_of_sight() {
        assert_eq!(out_of_sight(8, 5, 0.0), OutOfSight { left: 0, right: 3 });
        assert_eq!(out_of_sight(8, 5, 2.0), OutOfSight { left: 2, right: 1 });
        assert_eq!(out_of_sight(8, 5, 3.0), OutOfSight { left: 3, right: 0 });
        assert_eq!(out_of_sight(8, 5, 1.4), OutOfSight { left: 1, right: 2 });
        assert_eq!(out_of_sight(8, 5, 3.2), OutOfSight { left: 3, right: 0 });
        assert_eq!(out_of_sight(3, 5, 0.0), OutOfSight { left: 0, right: 0 });
    }

    #[test]
    fn the_wheel_scrolls_whole_icons_inside_the_row() {
        assert_eq!(row_width(5), px(5.0 * 28.0 + 4.0 * 2.0), "five icons show");
        assert_eq!(scrolled(px(0.0), -1, 8, 5), step(), "down scrolls on");
        assert_eq!(scrolled(px(0.0), 1, 8, 5), px(0.0), "not past the start");
        assert_eq!(scrolled(px(0.0), -9, 8, 5), step() * 3.0, "nor past the end");
        assert_eq!(scrolled(step() * 0.4, -1, 8, 5), step(), "from a scroll still running");
        assert_eq!(scrolled(px(0.0), -1, 3, 5), px(0.0), "nothing to scroll");
    }

    #[test]
    fn icons_fade_over_the_strip_s_ends_only_where_more_are() {
        let width = row_width(5);
        assert_eq!(edge_fade(px(0.0), width, false, true), 1.0);
        assert!(edge_fade(px(0.0), width, true, true) < 1.0);
        assert_eq!(edge_fade(width - PILL_BUTTON_SIZE, width, true, false), 1.0);
        assert!(edge_fade(width - PILL_BUTTON_SIZE, width, false, true) < 1.0);
    }

    fn menu_entry(id: i32, separator: bool) -> TrayMenuEntry {
        TrayMenuEntry {
            id,
            label: if separator { String::new() } else { format!("Entry {id}") },
            separator,
            enabled: true,
            toggle: Toggle::None,
            checked: false,
            submenu: false,
        }
    }

    #[test]
    fn a_menu_is_as_tall_as_its_rows_and_a_long_one_scrolls() {
        let entries = [menu_entry(1, false), menu_entry(2, true), menu_entry(3, false)];
        let (list, total) = menu_heights(&entries, false, px(1000.0));
        assert_eq!(list, MENU_ROW_HEIGHT * 2.0 + separator_height());
        assert_eq!(total, MENU_PAD * 2.0 + MENU_ROW_HEIGHT * 4.0 + separator_height() * 2.0);
        let (_, in_submenu) = menu_heights(&entries, true, px(1000.0));
        assert_eq!(in_submenu, total + MENU_ROW_HEIGHT, "with Back");
        let (_, empty) = menu_heights(&[], false, px(1000.0));
        assert_eq!(empty, MENU_PAD * 2.0 + MENU_ROW_HEIGHT * 2.0, "the app and Hide from tray");

        let long: Vec<TrayMenuEntry> = (0..40).map(|id| menu_entry(id, false)).collect();
        let (list, total) = menu_heights(&long, false, px(500.0));
        assert_eq!(total, px(500.0));
        assert!(list < entries_height(&long));
    }
}
