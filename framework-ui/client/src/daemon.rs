use crate::link::{Incoming, Link, Refusal};
use crate::routes::Routes;
use futures::channel::{mpsc, oneshot};
use futures::{FutureExt, StreamExt};
use gpui::{App, AppContext, Context, Entity, Global, SharedString, Subscription, Task};
use nova_carrier::Topic;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

pub const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Daemon {
    link: Rc<Link>,
    crossing: mpsc::UnboundedSender<Crossing>,
    connection: Entity<Connection>,
    routes: Rc<RefCell<Routes<App>>>,
}

impl Global for Daemon {}

enum Crossing {
    Message(Incoming),
    Answer(Box<dyn FnOnce() + Send>),
}

pub struct Connection {
    status: Status,
}

impl Connection {
    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.status, Status::Connected { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected { daemon: SharedString },
    Lost,
}

impl Daemon {
    pub fn connect(socket: PathBuf, cx: &mut App) {
        let (crossing, mut crossed) = mpsc::unbounded();
        let messages = crossing.clone();
        let link = Link::open(socket, move |message| {
            let _ = messages.unbounded_send(Crossing::Message(message));
        });
        cx.spawn(async move |cx| {
            while let Some(crossing) = crossed.next().await {
                match crossing {
                    Crossing::Message(message) => cx.update(|cx| Self::take(message, cx)),
                    Crossing::Answer(hand_over) => hand_over(),
                }
            }
        })
        .detach();
        let connection = cx.new(|_| Connection { status: Status::Connecting });
        cx.set_global(Self { link: Rc::new(link), crossing, connection, routes: Rc::default() });
    }

    pub fn connection(cx: &App) -> Option<Entity<Connection>> {
        cx.try_global::<Self>().map(|daemon| daemon.connection.clone())
    }

    pub fn on_event<E: DeserializeOwned + 'static>(
        topic: Topic,
        cx: &App,
        handler: impl Fn(E, &mut App) + 'static,
    ) -> Subscription {
        let Some(daemon) = cx.try_global::<Self>() else { return Subscription::new(|| {}) };
        let id = daemon.routes.borrow_mut().add(handler);
        daemon.link.subscribe(topic);
        let (routes, link) = (Rc::downgrade(&daemon.routes), Rc::downgrade(&daemon.link));
        Subscription::new(move || {
            if let Some(routes) = routes.upgrade() {
                routes.borrow_mut().remove(id);
            }
            if let Some(link) = link.upgrade() {
                link.unsubscribe(topic);
            }
        })
    }

    pub fn call<R: DeserializeOwned + 'static>(command: impl Serialize, cx: &App) -> Task<Result<R, Refusal>> {
        let (sender, answer) = oneshot::channel();
        let asked = cx.try_global::<Self>().and_then(|daemon| {
            let crossing = daemon.crossing.clone();
            let asked = daemon.link.request(command, move |result| {
                let hand_over = Box::new(move || {
                    let _ = sender.send(result);
                });
                let _ = crossing.unbounded_send(Crossing::Answer(hand_over));
            });
            asked.map(|asked| (Rc::downgrade(&daemon.link), asked))
        });
        cx.spawn(async move |cx| {
            let mut too_late = cx.background_executor().timer(REPLY_TIMEOUT).fuse();
            let result = futures::select_biased! {
                result = answer.fuse() => result.unwrap_or(Err(Refusal::NotConnected)),
                () = too_late => {
                    if let Some((link, asked)) = asked.and_then(|(link, asked)| Some((link.upgrade()?, asked))) {
                        link.forget(asked);
                    }
                    Err(Refusal::TimedOut)
                }
            }?;
            serde_json::from_value(result).map_err(|e| Refusal::Unreadable(format!("novad sent an odd result: {e}")))
        })
    }

    pub fn send(command: impl Serialize, cx: &App) {
        let name =
            serde_json::to_value(&command).ok().and_then(|command| Some(command.get("cmd")?.as_str()?.to_owned()));
        let name = name.unwrap_or_else(|| "command".into());
        let refused = move |result: Result<_, Refusal>| {
            if let Err(refusal) = result {
                crate::log::warn(&format!("{name} failed: {refusal}"));
            }
        };
        match cx.try_global::<Self>() {
            Some(daemon) => {
                daemon.link.request(command, refused);
            }
            None => refused(Err(Refusal::NotConnected)),
        }
    }

    fn take(message: Incoming, cx: &mut App) {
        let Some(daemon) = cx.try_global::<Self>() else { return };
        match message {
            Incoming::Connected { daemon: version } => {
                crate::log::info(&format!("connected to novad {version}"));
                Self::set_status(Status::Connected { daemon: version.into() }, cx);
            }
            Incoming::Lost => {
                crate::log::warn("novad went away, trying again");
                Self::set_status(Status::Lost, cx);
            }
            Incoming::Event { name, data } => {
                let (event, handlers) = daemon.routes.borrow().of(&name, data);
                for handler in handlers {
                    handler(&event, cx);
                }
            }
        }
    }

    fn set_status(status: Status, cx: &mut App) {
        let Some(connection) = Self::connection(cx) else { return };
        connection.update(cx, |connection, cx| {
            connection.status = status;
            cx.notify();
        });
    }
}

pub struct Live<S> {
    value: Option<S>,
    _events: Subscription,
}

impl<S: PartialEq + 'static> Live<S> {
    pub fn follow<E: DeserializeOwned + 'static>(
        topic: Topic,
        cx: &mut App,
        pick: impl Fn(E) -> Option<S> + 'static,
    ) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let this = cx.weak_entity();
            let events = Daemon::on_event(topic, cx, move |event: E, cx| {
                let Some(value) = pick(event) else { return };
                let _ = this.update(cx, |live, cx| live.set(value, cx));
            });
            Self { value: None, _events: events }
        })
    }

    pub fn get(&self) -> Option<&S> {
        self.value.as_ref()
    }

    fn set(&mut self, value: S, cx: &mut Context<Self>) {
        if self.value.as_ref() != Some(&value) {
            self.value = Some(value);
            cx.notify();
        }
    }
}
