//! Apps' tray items for tests, here and in novad's (behind the
//! `test-support` feature): items the way Electron (with `Activate`) and
//! libayatana (only a menu) put them up, served on a connection and
//! registered with the watcher.

use crate::{WATCHER_NAME, WATCHER_PATH};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use zbus::{Connection, interface};

/// An app's tray item, with the two properties the tray reads.
pub struct Item {
    pub id: String,
    pub title: String,
    /// Counts `Activate` calls.
    pub activated: Arc<std::sync::atomic::AtomicUsize>,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn id(&self) -> String {
        self.id.clone()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        self.title.clone()
    }

    fn activate(&self, _x: i32, _y: i32) {
        self.activated.fetch_add(1, Ordering::SeqCst);
    }
}

/// Serves `item` at `path` on `connection`, owning `name` first if given,
/// and registers `service` with the watcher.
pub fn serve_item(
    connection: &Connection,
    name: Option<&str>,
    path: &str,
    service: &str,
    item: impl zbus::object_server::Interface,
) {
    zbus::block_on(async {
        connection.object_server().at(path, item).await.unwrap();
        if let Some(name) = name {
            connection.request_name(name).await.unwrap();
        }
        register(connection, service).await.unwrap();
    });
}

pub(crate) async fn register(connection: &Connection, service: &str) -> zbus::Result<zbus::Message> {
    let method = "RegisterStatusNotifierItem";
    connection.call_method(Some(WATCHER_NAME), WATCHER_PATH, Some(WATCHER_NAME), method, &(service,)).await
}

/// A libayatana item, like Spotify's: no `Activate`, only a menu.
pub struct MenuOnlyItem {
    pub id: String,
    pub title: String,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl MenuOnlyItem {
    #[zbus(property)]
    fn id(&self) -> String {
        self.id.clone()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        self.title.clone()
    }

    fn secondary_activate(&self, _x: i32, _y: i32) {}
}

/// An item the way Electron puts one up for the GPUI UI's reads: pixels
/// rather than an icon name, a status, and a DBusMenu at `/MenuBar`.
pub struct LookItem {
    pub id: String,
    /// `Active` or `NeedsAttention`.
    pub status: String,
    /// One picture, as (width, height, ARGB32 bytes).
    pub pixmap: (i32, i32, Vec<u8>),
    /// Counts `SecondaryActivate` calls.
    pub secondary: Arc<std::sync::atomic::AtomicUsize>,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl LookItem {
    #[zbus(property)]
    fn id(&self) -> String {
        self.id.clone()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        self.status.clone()
    }

    #[zbus(property)]
    fn icon_name(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        vec![self.pixmap.clone()]
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> zbus::OwnedObjectPath {
        zbus::OwnedObjectPath::try_from("/MenuBar").unwrap()
    }

    fn activate(&self, _x: i32, _y: i32) {}

    fn secondary_activate(&self, _x: i32, _y: i32) {
        self.secondary.fetch_add(1, Ordering::SeqCst);
    }

    #[zbus(signal)]
    pub async fn new_status(emitter: &zbus::object_server::SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
}

/// One entry of a [`Menu`]: its id, properties and children.
pub struct MenuNode(pub i32, pub Vec<(&'static str, zbus::OwnedVariant)>, pub Vec<MenuNode>);

/// A DBusMenu, served as Electron's (`/MenuBar`) is.
pub struct Menu {
    pub root: MenuNode,
    pub revision: u32,
    /// The entries clicked, in order.
    pub clicked: Arc<std::sync::Mutex<Vec<i32>>>,
    /// The levels told they're about to show.
    pub shown: Arc<std::sync::Mutex<Vec<i32>>>,
}

/// An entry as `GetLayout` sends it.
pub type Layout = (i32, std::collections::HashMap<String, zbus::OwnedVariant>, Vec<zbus::OwnedVariant>);

/// `node` as `GetLayout` sends it, `depth` levels deep (-1: all of them).
fn layout(node: &MenuNode, depth: i32) -> Layout {
    let MenuNode(id, props, children) = node;
    let props = props.iter().map(|(name, value)| ((*name).to_string(), value.try_clone().unwrap())).collect();
    let children = if depth == 0 {
        Vec::new()
    } else {
        children
            .iter()
            .map(|child| zbus::OwnedVariant::try_from(zbus::Variant::from(layout(child, depth - 1))).unwrap())
            .collect()
    };
    (*id, props, children)
}

fn find(node: &MenuNode, id: i32) -> Option<&MenuNode> {
    if node.0 == id {
        return Some(node);
    }
    node.2.iter().find_map(|child| find(child, id))
}

#[interface(name = "com.canonical.dbusmenu")]
impl Menu {
    fn get_layout(&self, parent: i32, depth: i32, _names: Vec<String>) -> zbus::fdo::Result<(u32, Layout)> {
        let node =
            find(&self.root, parent).ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no entry {parent}")))?;
        Ok((self.revision, layout(node, depth)))
    }

    fn about_to_show(&self, id: i32) -> bool {
        self.shown.lock().unwrap().push(id);
        false
    }

    fn event(&self, id: i32, event: &str, _data: zbus::OwnedVariant, _timestamp: u32) {
        if event == "clicked" {
            self.clicked.lock().unwrap().push(id);
        }
    }

    #[zbus(signal)]
    pub async fn layout_updated(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}
