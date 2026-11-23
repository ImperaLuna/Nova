use super::*;

pub(crate) async fn serve(shared: &Arc<Shared>, ready: &std::sync::mpsc::Sender<()>) -> zbus::Result<()> {
    let server = Server { shared: Arc::clone(shared) };
    let connection = shared.sources.bus.builder().method_timeout(CALL_TIMEOUT).serve_at(PATH, server).build().await?;
    match session_bus::claim(&connection, BUS_NAME).await? {
        RequestNameReply::InQueue => shared.logger.warn(
            LOG_SOURCE,
            &format!("another program owns {BUS_NAME}; its notifications show instead until it quits"),
        ),
        _ => shared.logger.info(LOG_SOURCE, &format!("showing notifications as {BUS_NAME}")),
    }
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")
        .interface("org.freedesktop.DBus")
        .member("NameOwnerChanged")
        .build()?;
    let mut owners = MessageStream::for_match_rule(rule, &connection, None).await?;
    *lock(&shared.connection) = Some(connection.clone());
    let _ = ready.send(());
    let result = loop {
        let next = poll_fn(|cx| match shared.stop.poll(cx) {
            Poll::Ready(()) => Poll::Ready(None),
            Poll::Pending => Pin::new(&mut owners).poll_next(cx).map(Some),
        });
        let Some(Some(message)) = next.await else { break Ok(()) };
        let changed = message.and_then(|message| message.body().deserialize::<(String, String, String)>());
        match changed {
            Ok((name, _, owner)) if owner.is_empty() => shared.sender_gone(&name),
            Ok(_) => {}
            Err(e) => break Err(e),
        }
    };
    *lock(&shared.connection) = None;
    result
}

struct Server {
    shared: Arc<Shared>,
}

#[interface(name = "org.freedesktop.Notifications")]
impl Server {
    #[expect(clippy::too_many_arguments, reason = "the spec's signature")]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedVariant>,
        expire_timeout: i32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<u32> {
        let sent = Sent { app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout };
        let (id, gone) = self.shared.notify(sent, header.sender().map(|sender| sender.to_string()), None);
        for old in gone {
            Self::notification_closed(&emitter, old, Closed::Other as u32).await?;
        }
        Ok(id)
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let closed = {
            let mut state = lock(&self.shared.state);
            let live = find(&state, id).is_some_and(|kept| kept.live);
            if live {
                self.shared.retire(&mut state, id, false);
            }
            live
        };
        if closed {
            Self::notification_closed(&emitter, id, Closed::ByApp as u32).await?;
        }
        Ok(())
    }

    fn get_capabilities(&self) -> Vec<&'static str> {
        vec!["actions", "body", "body-markup", "icon-static", "persistence"]
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        ("Nova", "Nova", env!("CARGO_PKG_VERSION"), "1.2")
    }

    #[zbus(signal)]
    async fn notification_closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;
}
