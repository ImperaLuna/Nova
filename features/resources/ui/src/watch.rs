//! Whether this program watches the machine's resources: while any
//! screen's drawer shows the card or the page. novad samples only while
//! some connection watches, and a new connection watches nothing, so it is
//! said again each time novad comes back.

use gpui::{App, AppContext, DisplayId, Entity, Global, Subscription};
use nova_resources_api::Command;
use nova_ui_client::Daemon;
use std::collections::HashSet;

/// The screens whose drawer shows the resources now.
struct Watch {
    screens: HashSet<DisplayId>,
    _connection: Option<Subscription>,
}

struct Watching(Entity<Watch>);

impl Global for Watching {}

fn watch(cx: &mut App) -> Entity<Watch> {
    if let Some(watching) = cx.try_global::<Watching>() {
        return watching.0.clone();
    }
    let watch = cx.new(|cx| {
        // novad came back: it forgot, so say it again.
        let connection = Daemon::connection(cx).map(|connection| {
            cx.observe(&connection, |watch: &mut Watch, connection, cx| {
                if connection.read(cx).is_connected() && !watch.screens.is_empty() {
                    Daemon::send(Command::WatchResources { watching: true }, cx);
                }
            })
        });
        Watch { screens: HashSet::new(), _connection: connection }
    });
    cx.set_global(Watching(watch.clone()));
    watch
}

/// Says whether `screen`'s drawer shows the resources; novad hears when
/// the first screen starts or the last one stops.
pub fn set(screen: DisplayId, shows: bool, cx: &mut App) {
    watch(cx).update(cx, |watch, cx| {
        let before = !watch.screens.is_empty();
        match shows {
            true => watch.screens.insert(screen),
            false => watch.screens.remove(&screen),
        };
        let now = !watch.screens.is_empty();
        if now != before {
            Daemon::send(Command::WatchResources { watching: now }, cx);
        }
    });
}
