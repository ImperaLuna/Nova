use crate::Daemon;
use gpui::{App, AppContext, Entity, EntityId, Global, Subscription};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

type Send = Rc<dyn Fn(bool, &App)>;

pub struct Watch {
    wanting: HashSet<EntityId>,
    send: Send,
    _connection: Option<Subscription>,
}

#[derive(Default)]
struct Watches(HashMap<&'static str, Entity<Watch>>);

impl Global for Watches {}

impl Watch {
    pub fn named<C: Serialize + 'static>(name: &'static str, command: fn(bool) -> C, cx: &mut App) -> Entity<Self> {
        if let Some(watch) = cx.default_global::<Watches>().0.get(name) {
            return watch.clone();
        }
        let send: Send = Rc::new(move |wanted, cx| Daemon::send(command(wanted), cx));
        let watch = cx.new(|cx| {
            let connection = Daemon::connection(cx).map(|connection| {
                cx.observe(&connection, |watch: &mut Watch, connection, cx| {
                    watch.again(connection.read(cx).is_connected(), cx)
                })
            });
            Watch { wanting: HashSet::new(), send, _connection: connection }
        });
        cx.default_global::<Watches>().0.insert(name, watch.clone());
        watch
    }

    fn again(&self, connected: bool, cx: &App) {
        if connected && self.is_wanted() {
            (self.send)(true, cx);
        }
    }

    pub fn is_wanted(&self) -> bool {
        !self.wanting.is_empty()
    }

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
