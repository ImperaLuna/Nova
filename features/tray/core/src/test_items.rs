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
