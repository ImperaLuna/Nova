//! Runs the tray feature (`nova-tray`, in `features/tray`) in novad: its
//! module and the answers to its commands. Wiring only: it adapts the
//! feature's core to novad's `Module` trait and `Daemon`, and all of the
//! tray's work and state stay in the core. A click on an item, or a pick
//! from its menu, also focuses its app's window, which the launcher and the
//! compositor do: novad joins them here until the launcher has moved too.

use crate::Daemon;
use nova_hub::ClientId;
use nova_protocol::Outcome;
use nova_socket::{Answer, Caller, done};
pub use nova_tray::Sources;
use nova_tray_api::Command;
use std::sync::Arc;

pub use nova_tray::{FEATURE_ID, candidates};

/// The feature's core, as novad's module.
pub struct Tray(Arc<nova_tray::Tray>);

impl std::ops::Deref for Tray {
    type Target = nova_tray::Tray;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The answer to one of the feature's commands from `caller`, the route
/// `requests::router` gives them.
pub fn answer(daemon: &Daemon, _caller: &Caller, command: &Command) -> Answer {
    match command {
        Command::HideTrayApp { id, hidden } => {
            Answer::Now(done(daemon.module::<Tray>().and_then(|tray| tray.hide(id, *hidden))))
        }
        Command::ActivateTrayItem { address } => activate_item(daemon, address),
        Command::TrayMenuPicked { address } => menu_picked(daemon, address),
    }
}

/// A click on the tray item at `address`: focuses the window its app
/// has open, then asks the item to show (or hide) its window
/// (`Activate`). Apps get no activation token from a tray click, so niri
/// wouldn't move focus to it themselves. An app with no window open is
/// focused once its window shows, or else started again and focused once
/// its window shows, as the launcher does: Claude Desktop ignores
/// `Activate` but shows its window when started again. An open window
/// means nothing starts, so an icon that hides its app on a click still
/// can.
fn activate_item(daemon: &Daemon, address: &str) -> Answer {
    let tray = match daemon.module::<Tray>() {
        Ok(tray) => tray,
        Err(e) => return Answer::Now(Outcome::Error(e)),
    };
    let launcher = daemon.module::<crate::launcher::Launcher>().ok();
    let compositor = daemon.compositor.clone();
    let logger = Arc::clone(&daemon.logger);
    let address = address.to_owned();
    Answer::later(move || {
        done(tray.app_at(&address).and_then(|app| {
            let found = app
                .zip(launcher.as_ref())
                .and_then(|(app, launcher)| Some((launcher, launcher.window_ids(&app)?, app)));
            let (Some((launcher, ids, app)), Some(compositor)) = (found, compositor.as_deref()) else {
                return tray.activate(&address);
            };
            // A terminal app's window can't be told apart; an error can't
            // tell whether one is open, so nothing starts either way.
            let open = ids.is_empty() || compositor.focus_app(&ids, None).unwrap_or(true);
            tray.activate(&address)?;
            let shown = |wait| crate::launcher::focus_shown(&app, &ids, compositor, None, wait, &logger);
            if open || shown(crate::launcher::SHOWN_WINDOW_WAIT) {
                return Ok(());
            }
            launcher.start_app(&app)?;
            shown(crate::launcher::STARTED_WINDOW_WAIT);
            Ok(())
        }))
    })
}

/// An entry of the menu of the tray item at `address` is about to run:
/// when its app has no window open now, one that shows within a while
/// takes focus (the menu's "Show" entry), as the app gets no activation
/// token from a menu either. An open window is left be: the entry may
/// be anything, like pausing a song. Answers once it has looked, so the
/// UI runs the entry after, and a window it shows can't be taken for
/// one that was open.
fn menu_picked(daemon: &Daemon, address: &str) -> Answer {
    let found = daemon.module::<Tray>().and_then(|tray| tray.app_at(address)).map(|app| {
        let launcher = daemon.module::<crate::launcher::Launcher>().ok()?;
        let app = app?;
        Some((launcher.window_ids(&app)?, app))
    });
    let (ids, app) = match found {
        Ok(Some(found)) => found,
        Ok(None) => return Answer::Now(Outcome::Result(serde_json::Value::Null)),
        Err(e) => return Answer::Now(Outcome::Error(e)),
    };
    let Some(compositor) = daemon.compositor.clone() else {
        return Answer::Now(Outcome::Result(serde_json::Value::Null));
    };
    if compositor.app_workspace(&ids).is_ok_and(|workspace| workspace.is_none()) {
        let logger = Arc::clone(&daemon.logger);
        std::thread::spawn(move || {
            crate::launcher::focus_shown(&app, &ids, &compositor, None, crate::launcher::SHOWN_WINDOW_WAIT, &logger)
        });
    }
    Answer::Now(Outcome::Result(serde_json::Value::Null))
}

/// Started before the UI, which settings start ahead of, so apps it
/// autostarts find the watcher. No options.
impl crate::modules::Module for Tray {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const NEEDS_APPS: bool = true;
    const TOPIC: Option<nova_protocol::Topic> = Some(nova_tray_api::TOPIC);
    type Sources = Sources;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> Sources {
        Sources::new(&env.args.state_dir)
    }

    fn config(_: &crate::config::Config) {}

    fn start(
        ctx: &crate::modules::Context,
        sources: &Sources,
        (): &(),
        _: &[nova_protocol::FeatureState],
    ) -> Arc<Self> {
        Arc::new(Self(nova_tray::Tray::start(
            sources.clone(),
            Arc::clone(&ctx.apps),
            Arc::clone(&ctx.hub),
            Arc::clone(&ctx.logger),
        )))
    }

    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn stop(&self) {
        self.0.stop();
    }
}
