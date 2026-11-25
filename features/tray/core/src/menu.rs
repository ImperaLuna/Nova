use nova_tray_api::{Toggle, TrayMenuEntry, TrayMenuLevel};
use std::collections::HashMap;
use zbus::Connection;
use zbus::{OwnedVariant as OwnedValue, Structure, Variant as Value};

pub const MENU_INTERFACE: &str = "com.canonical.dbusmenu";
const MAX_ENTRIES: usize = 256;
const MAX_LABEL_CHARS: usize = 256;

type Node = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

pub async fn level(connection: &Connection, bus: &str, path: &str, id: i32) -> zbus::Result<TrayMenuLevel> {
    let _ = connection.call_method(Some(bus), path, Some(MENU_INTERFACE), "AboutToShow", &(id,)).await;
    let no_names: &[&str] = &[];
    let reply =
        connection.call_method(Some(bus), path, Some(MENU_INTERFACE), "GetLayout", &(id, 1i32, no_names)).await?;
    let (revision, (_, _, children)): (u32, Node) = reply.body().deserialize()?;
    Ok(TrayMenuLevel { revision, entries: entries(children) })
}

pub async fn revision(connection: &Connection, bus: &str, path: &str) -> zbus::Result<u32> {
    let no_names: &[&str] = &[];
    let reply =
        connection.call_method(Some(bus), path, Some(MENU_INTERFACE), "GetLayout", &(0i32, 0i32, no_names)).await?;
    let (revision, _): (u32, Node) = reply.body().deserialize()?;
    Ok(revision)
}

pub async fn click(connection: &Connection, bus: &str, path: &str, id: i32) -> zbus::Result<()> {
    let data = Value::from(0i32);
    connection.call_method(Some(bus), path, Some(MENU_INTERFACE), "Event", &(id, "clicked", data, 0u32)).await?;
    Ok(())
}

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
