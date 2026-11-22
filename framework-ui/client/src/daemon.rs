//! The link brought to GPUI: novad's lines cross from the link's thread to
//! GPUI's, where each event goes to the handlers of its type and entities
//! tell their views. Everything here runs on GPUI's thread.

use crate::link::{Incoming, Link, Refusal};
use crate::routes::Routes;
use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use gpui::{App, AppContext, Context, Entity, Global, SharedString, Subscription, Task};
use nova_carrier::Topic;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// The one connection to novad, a GPUI global set by [`Daemon::connect`].
/// Without it (a view shown on its own) nothing arrives and every command
/// is refused.
pub struct Daemon {
    link: Link,
    /// The way onto GPUI's thread that novad's lines take, for answers.
    crossing: mpsc::UnboundedSender<Crossing>,
    connection: Entity<Connection>,
    routes: Rc<RefCell<Routes<App>>>,
}

impl Global for Daemon {}

/// What crosses from the link's thread to GPUI's, in the order novad sent
/// it: an answer never overtakes the events novad sent before it.
enum Crossing {
    Message(Incoming),
    /// Hands a request's result to whoever waits for it.
    Answer(Box<dyn FnOnce() + Send>),
}

/// Whether novad is there, for views that show it and state that must not
/// outlive a connection.
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
    /// novad hasn't answered yet.
    Connecting,
    /// Connected to a novad of this version.
    Connected { daemon: SharedString },
    /// novad went away. The link keeps trying, and what was shown stays.
    Lost,
}

impl Daemon {
    /// Connects to novad at `socket`, and again whenever it goes away.
    pub fn connect(socket: PathBuf, cx: &mut App) {
        let (crossing, mut crossed) = mpsc::unbounded();
        let messages = crossing.clone();
        let link = Link::open(socket, move |message| {
            // Nobody takes them once the app is gone.
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
        cx.set_global(Self { link, crossing, connection, routes: Rc::default() });
    }

    /// Whether novad is there. `None` when the app never connected.
    pub fn connection(cx: &App) -> Option<Entity<Connection>> {
        cx.try_global::<Self>().map(|daemon| daemon.connection.clone())
    }

    /// Calls `handler` with each event of the type `E` (a feature's `Event`
    /// enum) novad sends on `topic`, until the subscription is dropped.
    /// novad sends a topic's state right as it is subscribed to: to a new
    /// handler, and on every connection.
    ///
    /// Panics when `E` isn't an enum tagged `event`, as every api crate's
    /// `Event` is: its names are what routes novad's events to it.
    pub fn on_event<E: DeserializeOwned + 'static>(
        topic: Topic,
        cx: &App,
        handler: impl Fn(E, &mut App) + 'static,
    ) -> Subscription {
        let Some(daemon) = cx.try_global::<Self>() else { return Subscription::new(|| {}) };
        let id = daemon.routes.borrow_mut().add(handler);
        daemon.link.subscribe(topic);
        let routes = Rc::downgrade(&daemon.routes);
        Subscription::new(move || {
            if let Some(routes) = routes.upgrade() {
                routes.borrow_mut().remove(id);
            }
        })
    }

    /// Sends `command` (a feature's `Command`, or novad's own) and gives
    /// its result as `R`. The result arrives after every event novad sent
    /// before it.
    pub fn call<R: DeserializeOwned + 'static>(command: impl Serialize, cx: &App) -> Task<Result<R, Refusal>> {
        let (sender, answer) = oneshot::channel();
        if let Some(daemon) = cx.try_global::<Self>() {
            let crossing = daemon.crossing.clone();
            daemon.link.request(command, move |result| {
                let hand_over = Box::new(move || {
                    // Nobody waits once the task is dropped.
                    let _ = sender.send(result);
                });
                let _ = crossing.unbounded_send(Crossing::Answer(hand_over));
            });
        }
        cx.spawn(async move |_| {
            let result = answer.await.unwrap_or(Err(Refusal::NotConnected))?;
            serde_json::from_value(result).map_err(|e| Refusal::Unreadable(format!("novad sent an odd result: {e}")))
        })
    }

    /// Sends `command` when nothing waits for its result. A refusal is
    /// logged, so a failed command is never silent.
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
            Some(daemon) => daemon.link.request(command, refused),
            None => refused(Err(Refusal::NotConnected)),
        }
    }

    /// One message from the link, on GPUI's thread.
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

/// The latest state novad sent on a topic: `None` until the first one
/// arrives (the feature may be off), then always the last, also while novad
/// is away. Views observe the entity and read [`Live::get`].
pub struct Live<S> {
    value: Option<S>,
    _events: Subscription,
}

impl<S: PartialEq + 'static> Live<S> {
    /// Follows `topic`: `pick` takes the state out of each event of the
    /// topic's type `E`, or gives `None` for an event that carries none.
    pub fn follow<E: DeserializeOwned + 'static>(
        topic: Topic,
        cx: &mut App,
        pick: impl Fn(E) -> Option<S> + 'static,
    ) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let this = cx.weak_entity();
            let events = Daemon::on_event(topic, cx, move |event: E, cx| {
                let Some(value) = pick(event) else { return };
                // Gone with its last view: the subscription goes with it.
                let _ = this.update(cx, |live, cx| live.set(value, cx));
            });
            Self { value: None, _events: events }
        })
    }

    pub fn get(&self) -> Option<&S> {
        self.value.as_ref()
    }

    /// Views are told only of a change: novad sends the state again on
    /// every connection.
    fn set(&mut self, value: S, cx: &mut Context<Self>) {
        if self.value.as_ref() != Some(&value) {
            self.value = Some(value);
            cx.notify();
        }
    }
}
