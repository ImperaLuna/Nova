//! The watcher on the session bus: claiming its name, the apps that
//! register with it and leave, the signals that say an item's look or menu
//! changed, and the calls novad makes to items.

use super::*;

/// Connects to `bus`, claims the watcher's name and serves it, dropping
/// items whose app leaves the bus, until `stop` or the bus goes. Sends on
/// `ready` once apps can register.
pub(crate) async fn serve(shared: &Arc<Shared>, bus: Bus, ready: &std::sync::mpsc::Sender<()>) -> zbus::Result<()> {
    let builder = bus.builder();
    let watcher = Watcher { shared: Arc::clone(shared) };
    let connection = builder.method_timeout(CALL_TIMEOUT).serve_at(WATCHER_PATH, watcher).build().await?;
    // Followed before the name is claimed, so no app leaving in between is missed.
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(DBUS_NAME)
        .interface(DBUS_NAME)
        .member("NameOwnerChanged")
        .build()?;
    let mut owners = MessageStream::for_match_rule(rule, &connection, None).await?;
    follow_looks(shared, &connection).await?;
    // Queued behind another watcher (another shell's, say), novad takes over when it leaves.
    match session_bus::claim(&connection, WATCHER_NAME).await? {
        RequestNameReply::InQueue => shared.logger.warn(
            LOG_SOURCE,
            &format!("another program owns {WATCHER_NAME}; tray icons show through it until it quits"),
        ),
        _ => shared.logger.info(LOG_SOURCE, &format!("watching for tray icons as {WATCHER_NAME}")),
    }
    *lock(&shared.connection) = Some(connection.clone());
    let _ = ready.send(());
    let result = follow_owners(shared, &connection, &mut owners).await;
    // The watcher it serves holds `shared`: kept, the connection would never
    // close, and a stopped tray would keep the name from the next one.
    *lock(&shared.connection) = None;
    result
}

/// The signals an item sends when what it shows changed.
pub(crate) const LOOK_SIGNALS: [&str; 5] = ["NewIcon", "NewAttentionIcon", "NewStatus", "NewIconThemePath", "NewMenu"];

/// Follows items' signals that their look changed, and menus' that their
/// layout did, on a thread of their own, until `stop` or the bus goes.
pub(crate) async fn follow_looks(shared: &Arc<Shared>, connection: &Connection) -> zbus::Result<()> {
    let signals = |interface: &'static str| {
        MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(interface).build()
    };
    let mut items = MessageStream::for_match_rule(signals(ITEM_INTERFACE)?, connection, None).await?;
    let mut menus = MessageStream::for_match_rule(signals(menu::MENU_INTERFACE)?, connection, None).await?;
    let (following, connection) = (Arc::clone(shared), connection.clone());
    std::thread::spawn(move || {
        zbus::block_on(async {
            while let Some(Ok(message)) = next_unless_stopped(&mut items, &following.stop).await {
                let header = message.header();
                let (Some(sender), Some(path), Some(member)) = (header.sender(), header.path(), header.member()) else {
                    continue;
                };
                if LOOK_SIGNALS.contains(&member.as_str()) {
                    following.look_changed(&connection, sender.as_str(), path.as_str()).await;
                }
            }
        });
    });
    let following = Arc::clone(shared);
    std::thread::spawn(move || {
        zbus::block_on(async {
            while let Some(Ok(message)) = next_unless_stopped(&mut menus, &following.stop).await {
                let header = message.header();
                let (Some(sender), Some(path)) = (header.sender(), header.path()) else { continue };
                if header.member().is_some_and(|member| member.as_str() == "LayoutUpdated")
                    && let Ok((revision, _parent)) = message.body().deserialize::<(u32, i32)>()
                {
                    following.saw_revision(sender.as_str(), path.as_str(), revision);
                }
            }
        });
    });
    Ok(())
}

/// Drops the items of apps that leave the bus, until `stop` or the bus goes.
pub(crate) async fn follow_owners(
    shared: &Shared,
    connection: &Connection,
    owners: &mut MessageStream,
) -> zbus::Result<()> {
    let emitter = SignalEmitter::new(connection, WATCHER_PATH)?;
    while let Some(message) = next_unless_stopped(owners, &shared.stop).await {
        let (name, old, new): (String, String, String) = message?.body().deserialize()?;
        let gone = match (old.is_empty(), new.is_empty()) {
            (_, true) => shared.drop_bus(&name),
            (false, false) => shared.drop_handed_over(&name, &old),
            (true, false) => continue,
        };
        for address in gone {
            emitter.status_notifier_item_unregistered(&address).await?;
        }
    }
    Ok(())
}

/// The stream's next message; `None` once the connection closes or `stop` is set.
pub(crate) async fn next_unless_stopped(
    stream: &mut MessageStream,
    stop: &Stop,
) -> Option<zbus::Result<zbus::Message>> {
    poll_fn(|cx| {
        if stop.poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Pin::new(&mut *stream).poll_next(cx)
    })
    .await
}

/// Whether `e` is an app's answer that it has no such item, rather than
/// no answer in time or a passing failure.
pub(crate) fn says_not_there(e: &zbus::Error) -> bool {
    matches!(
        error_name(e).as_deref().unwrap_or_default(),
        "org.freedesktop.DBus.Error.UnknownObject"
            | "org.freedesktop.DBus.Error.UnknownInterface"
            | "org.freedesktop.DBus.Error.UnknownMethod"
            | "org.freedesktop.DBus.Error.UnknownProperty"
    )
}

/// The name of the D-Bus error an app answered with, if it answered.
pub(crate) fn error_name(e: &zbus::Error) -> Option<String> {
    use zbus::DBusError;
    match e {
        zbus::Error::MethodError(name, ..) => Some(name.to_string()),
        zbus::Error::FDO(e) => Some(e.name().to_string()),
        _ => None,
    }
}

/// Tells hosts the items at `addresses` went.
pub(crate) async fn unregister(connection: &Connection, addresses: Vec<String>) {
    let Ok(emitter) = SignalEmitter::new(connection, WATCHER_PATH) else { return };
    for address in addresses {
        let _ = emitter.status_notifier_item_unregistered(&address).await;
    }
}

pub(crate) async fn name_owner(connection: &Connection, name: &str) -> zbus::Result<String> {
    let reply = connection.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "GetNameOwner", &(name,)).await?;
    reply.body().deserialize()
}

/// A string property of the item at `bus` and `path`, cut to [`MAX_TEXT_CHARS`].
pub(crate) async fn item_text(connection: &Connection, bus: &str, path: &str, name: &str) -> zbus::Result<String> {
    let reply = connection.call_method(Some(bus), path, Some(PROPERTIES), "Get", &(ITEM_INTERFACE, name)).await?;
    let text: String = reply.body().deserialize::<OwnedVariant>()?.try_into()?;
    Ok(text.chars().take(MAX_TEXT_CHARS).collect())
}

/// The properties the item at `bus` and `path` shows itself with, or
/// `None` when it doesn't answer.
pub(crate) async fn item_props(connection: &Connection, bus: &str, path: &str) -> Option<look::Props> {
    let reply = connection.call_method(Some(bus), path, Some(PROPERTIES), "GetAll", &(ITEM_INTERFACE,)).await;
    reply.and_then(|reply| reply.body().deserialize::<HashMap<String, OwnedValue>>()).map(look::Props::from_all).ok()
}

/// Whether the item at `bus` and `path` has `Activate`. libayatana's items
/// (Spotify's, Steam's) only have a menu, and say so when introspected;
/// an item that can't be introspected (Electron's) is taken to have it,
/// and `Tray::send_activate` finds out otherwise.
pub(crate) async fn can_activate(connection: &Connection, bus: &str, path: &str) -> bool {
    let Ok(reply) = connection.call_method(Some(bus), path, Some(INTROSPECTABLE), "Introspect", &()).await else {
        return true;
    };
    reply.body().deserialize::<String>().map_or(true, |xml| has_method(&xml, ITEM_INTERFACE, "Activate") != Some(false))
}

/// Whether the introspection data `xml` gives `interface` the method
/// `method`; `None` when it doesn't list the interface.
pub(crate) fn has_method(xml: &str, interface: &str, method: &str) -> Option<bool> {
    let quoted = |name: &str| [format!("name=\"{name}\""), format!("name='{name}'")];
    let start = quoted(interface).iter().filter_map(|attribute| xml.find(attribute.as_str())).min()?;
    let body = &xml[start..];
    let body = &body[..body.find("</interface>").unwrap_or(body.len())];
    Some(body.split("<method").skip(1).any(|tag| {
        let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
        quoted(method).iter().any(|attribute| tag.contains(attribute.as_str()))
    }))
}

/// The watcher apps see on the bus.
pub(crate) struct Watcher {
    shared: Arc<Shared>,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let sender = header.sender().map(|sender| sender.to_string()).unwrap_or_default();
        let (address, new) = self.shared.register(connection, &sender, service).await.map_err(|e| {
            self.shared.logger.warn(LOG_SOURCE, &format!("refused a tray icon from {sender}: {e}"));
            zbus::fdo::Error::InvalidArgs(e)
        })?;
        emitter.status_notifier_item_registered(&address).await?;
        // Read once, by the first registration: an app registering again
        // doesn't get another thread.
        if !new {
            return Ok(());
        }
        // On a thread of its own, so the app gets its answer first and a slow
        // one holds up nobody else.
        let shared = Arc::clone(&self.shared);
        let connection = connection.clone();
        std::thread::spawn(move || zbus::block_on(shared.read(&connection, &address)));
        Ok(())
    }

    /// Hosts are welcome, but the watcher says there's one whatever they do.
    async fn register_status_notifier_host(
        &self,
        _service: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        emitter.status_notifier_host_registered().await?;
        Ok(())
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.shared.addresses()
    }

    /// Always: Nova's UI is the host, and it may still be starting.
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
