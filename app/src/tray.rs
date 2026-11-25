use crate::Daemon;
use nova_carrier::Outcome;
use nova_hub::ClientId;
use nova_socket::{Answer, Caller, done};
use nova_tray::Sources;
use nova_tray_api::Command;
use std::sync::Arc;

use nova_tray::FEATURE_ID;

pub struct Tray(Arc<nova_tray::Tray>);

impl std::ops::Deref for Tray {
    type Target = nova_tray::Tray;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn answer(daemon: &Daemon, _caller: &Caller, command: &Command) -> Answer {
    match command {
        Command::HideTrayApp { id, hidden } => {
            Answer::Now(done(daemon.module::<Tray>().and_then(|tray| tray.hide(id, *hidden))))
        }
        Command::ActivateTrayItem { address } => activate_item(daemon, address),
        Command::TrayMenuPicked { address } => menu_picked(daemon, address),
        Command::SecondaryActivateTrayItem { address } => {
            let tray = daemon.module::<Tray>();
            let address = address.clone();
            Answer::later(move || done(tray.and_then(|tray| tray.secondary_activate(&address))))
        }
        Command::TrayMenu { address, id } => {
            let tray = daemon.module::<Tray>();
            let (address, id) = (address.clone(), *id);
            Answer::later(move || match tray.and_then(|tray| tray.menu_level(&address, id)) {
                Ok(level) => Outcome::Result(serde_json::to_value(level).unwrap_or_default()),
                Err(e) => Outcome::Error(e),
            })
        }
        Command::RunTrayMenuEntry { address, id, revision } => run_menu_entry(daemon, address, *id, *revision),
    }
}

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
            let open = ids.is_empty() || compositor.focus_app(&ids, None).unwrap_or(true);
            tray.activate(&address)?;
            let shown = |wait| {
                nova_launcher::focus_shown(
                    nova_launcher::AppWindows { id: &app, ids: &ids },
                    compositor,
                    None,
                    wait,
                    &logger,
                )
            };
            if open || shown(nova_launcher::SHOWN_WINDOW_WAIT) {
                return Ok(());
            }
            launcher.start_app(&app)?;
            shown(nova_launcher::STARTED_WINDOW_WAIT);
            Ok(())
        }))
    })
}

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
            nova_launcher::focus_shown(
                nova_launcher::AppWindows { id: &app, ids: &ids },
                &compositor,
                None,
                nova_launcher::SHOWN_WINDOW_WAIT,
                &logger,
            )
        });
    }
    Answer::Now(Outcome::Result(serde_json::Value::Null))
}

fn run_menu_entry(daemon: &Daemon, address: &str, id: i32, revision: u32) -> Answer {
    if let Answer::Now(Outcome::Error(e)) = menu_picked(daemon, address) {
        return Answer::Now(Outcome::Error(e));
    }
    let tray = daemon.module::<Tray>();
    let address = address.to_owned();
    Answer::later(move || done(tray.and_then(|tray| tray.run_menu_entry(&address, id, revision))))
}

impl nova_modules::Module for Tray {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const NEEDS_APPS: bool = true;
    const TOPIC: Option<nova_carrier::Topic> = Some(nova_tray_api::TOPIC);
    type Sources = Sources;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> Sources {
        Sources::new(&env.args.state_dir, env.runtime_dir, Arc::clone(env.icons))
    }

    fn config(_: &crate::config::Config) {}

    fn start(
        ctx: &crate::modules::Context,
        sources: &Sources,
        (): &(),
        _: &[nova_daemon_info::FeatureState],
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
