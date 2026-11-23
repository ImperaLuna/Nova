//! Something novad does only while a client asks, which several views
//! share: Bluetooth's look for devices while its page shows on some screen,
//! the network's traffic each second while the network page shows.
//!
//! novad keeps such a request per connection, as one flag: the views of
//! every screen share one connection, so the page on one screen closing
//! must not stop what another screen's page still shows. A [`Watch`] is that
//! flag for all of them, by name: novad is told `true` when the first view
//! wants it, `false` when the last one stops (or is dropped), and `true`
//! again after it comes back, since a new connection watches nothing.
//! Views that act on it (Bluetooth's pairing pop-up stays away while the
//! page asks there) observe the entity and read [`Watch::is_wanted`].

use crate::Daemon;
use gpui::{App, AppContext, Entity, EntityId, Global, Subscription};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

type Send = Rc<dyn Fn(bool, &App)>;

/// A request several views share (see the module).
pub struct Watch {
    wanting: HashSet<EntityId>,
    send: Send,
    _connection: Option<Subscription>,
}

/// Every watch, by name.
#[derive(Default)]
struct Watches(HashMap<&'static str, Entity<Watch>>);

impl Global for Watches {}

impl Watch {
    /// The watch called `name`, made on first use: it sends `command(true)`
    /// or `command(false)` (the feature's watch command) as views come and
    /// go.
    pub fn named<C: Serialize + 'static>(name: &'static str, command: fn(bool) -> C, cx: &mut App) -> Entity<Self> {
        if let Some(watch) = cx.default_global::<Watches>().0.get(name) {
            return watch.clone();
        }
        let send: Send = Rc::new(move |wanted, cx| Daemon::send(command(wanted), cx));
        let watch = cx.new(|cx| {
            let connection = Daemon::connection(cx).map(|connection| {
                cx.observe(&connection, |watch: &mut Watch, connection, cx| watch.again(connection.read(cx).is_connected(), cx))
            });
            Watch { wanting: HashSet::new(), send, _connection: connection }
        });
        cx.default_global::<Watches>().0.insert(name, watch.clone());
        watch
    }

    /// novad came back: it watches nothing on a new connection.
    fn again(&self, connected: bool, cx: &App) {
        if connected && self.is_wanted() {
            (self.send)(true, cx);
        }
    }

    /// Whether some view wants it now.
    pub fn is_wanted(&self) -> bool {
        !self.wanting.is_empty()
    }

    /// The view `who` wants it or not. A view that may be dropped while it
    /// wants it says `false` in its `on_release`.
    pub fn want(watch: &Entity<Self>, who: EntityId, wanted: bool, cx: &mut App) {
        watch.update(cx, |watch, cx| {
            let was = watch.is_wanted();
            let changed = if wanted { watch.wanting.insert(who) } else { watch.wanting.remove(&who) };
            if changed && watch.is_wanted() != was {
                (watch.send)(watch.is_wanted(), cx);
                cx.notify();
            }
        });
    }
}
