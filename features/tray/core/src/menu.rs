//! A tray app's menu, as its `com.canonical.dbusmenu` object serves it:
//! one level at a time for the UI (`GetLayout` one deep, after
//! `AboutToShow` so the app can fill it), and an entry's click (`Event`).

use nova_tray_api::{Toggle, TrayMenuEntry, TrayMenuLevel};
use std::collections::HashMap;
use zbus::Connection;
use zbus::{OwnedVariant as OwnedValue, Structure, Variant as Value};

pub const MENU_INTERFACE: &str = "com.canonical.dbusmenu";
/// Entries one level lists at most; past it, the rest are left out.
const MAX_ENTRIES: usize = 256;
/// Labels are cut to this many characters.
const MAX_LABEL_CHARS: usize = 256;

/// An entry as `GetLayout` sends it: its id, properties and children.
type Node = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

/// Reads the level under entry `id` of the menu at `path` on `bus`.
pub async fn level(connection: &Connection, bus: &str, path: &str, id: i32) -> zbus::Result<TrayMenuLevel> {
    // Whether the app wants the layout read again doesn't matter: it is
    // read right after. An app without AboutToShow still has a layout.
    let _ = connection.call_method(Some(bus), path, Some(MENU_INTERFACE), "AboutToShow", &(id,)).await;
    let no_names: &[&str] = &[];
    let reply =
        connection.call_method(Some(bus), path, Some(MENU_INTERFACE), "GetLayout", &(id, 1i32, no_names)).await?;
    let (revision, (_, _, children)): (u32, Node) = reply.body().deserialize()?;
    Ok(TrayMenuLevel { revision, entries: entries(children) })
}

/// The menu's revision now: its layout, without entries.
pub async fn revision(connection: &Connection, bus: &str, path: &str) -> zbus::Result<u32> {
    let no_names: &[&str] = &[];
    let reply =
        connection.call_method(Some(bus), path, Some(MENU_INTERFACE), "GetLayout", &(0i32, 0i32, no_names)).await?;
    let (revision, _): (u32, Node) = reply.body().deserialize()?;
    Ok(revision)
}

/// Clicks entry `id` of the menu at `path` on `bus`.
pub async fn click(connection: &Connection, bus: &str, path: &str, id: i32) -> zbus::Result<()> {
    // Timestamp 0: the app has nothing to compare it with anyway.
    let data = Value::from(0i32);
    connection.call_method(Some(bus), path, Some(MENU_INTERFACE), "Event", &(id, "clicked", data, 0u32)).await?;
    Ok(())
}

/// The entries that show of a level's children, in order.
pub fn entries(children: Vec<OwnedValue>) -> Vec<TrayMenuEntry> {
    children.into_iter().filter_map(node).filter_map(entry).take(MAX_ENTRIES).collect()
}

fn node(child: OwnedValue) -> Option<Node> {
    let structure = Structure::try_from(child).ok()?;
    let mut fields = structure.into_fields().into_iter();
    let (id, props, children) = (fields.next()?, fields.next()?, fields.next()?);
    let id = i32::try_from(id).ok()?;
    let props = HashMap::<String, OwnedValue>::try_from(props).ok()?;
    let children = Vec::<OwnedValue>::try_from(children).unwrap_or_default();
    Some((id, props, children))
}

/// An entry from its properties, as the DBusMenu spec defaults them;
/// `None` for one that doesn't show.
fn entry((id, props, children): Node) -> Option<TrayMenuEntry> {
    let text = |name: &str| props.get(name).and_then(|value| String::try_from(value.clone()).ok());
    let flag = |name: &str| props.get(name).and_then(|value| bool::try_from(value).ok());
    if flag("visible") == Some(false) {
        return None;
    }
    let separator = text("type").as_deref() == Some("separator");
    let toggle = match text("toggle-type").as_deref() {
        Some("checkmark") => Toggle::Checkmark,
        Some("radio") => Toggle::Radio,
        _ => Toggle::None,
    };
    let state = props.get("toggle-state").and_then(|value| i32::try_from(value).ok());
    Some(TrayMenuEntry {
        id,
        label: if separator {
            String::new()
        } else {
            text("label").unwrap_or_default().chars().take(MAX_LABEL_CHARS).collect()
        },
        separator,
        enabled: flag("enabled").unwrap_or(true),
        toggle: if separator { Toggle::None } else { toggle },
        checked: toggle != Toggle::None && state == Some(1),
        submenu: !separator && (text("children-display").as_deref() == Some("submenu") || !children.is_empty()),
    })
}
