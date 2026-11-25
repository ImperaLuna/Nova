use crate::Daemon;
use nova_carrier::Outcome;
use nova_hub::ClientId;
use nova_notifications::Sources;
use nova_notifications_api::Command;
use nova_socket::{Answer, Caller, done};
use std::sync::Arc;

use nova_notifications::FEATURE_ID;

pub struct Notifications(Arc<nova_notifications::Notifications>);

impl Notifications {
    pub fn core(&self) -> &Arc<nova_notifications::Notifications> {
        &self.0
    }
}

impl std::ops::Deref for Notifications {
    type Target = nova_notifications::Notifications;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    let notifications = daemon.module::<Notifications>();
    Answer::Now(match command {
        Command::OpenNotification { id, output } => return open(daemon, *id, output.clone()),
        Command::NotificationAction { id, action } => {
            done(notifications.and_then(|notifications| notifications.action(*id, Some(action))))
        }
        Command::DismissNotification { id } => done(notifications.map(|notifications| notifications.dismiss(*id))),
        Command::ClearNotifications { app } => {
            done(notifications.map(|notifications| notifications.clear(app.as_deref())))
        }
        Command::ReadNotifications => done(notifications.map(|notifications| notifications.read(None))),
        Command::SetDoNotDisturb { on } => {
            done(notifications.map(|notifications| notifications.set_do_not_disturb(*on)))
        }
        Command::MuteNotificationApp { app, muted } => {
            done(notifications.and_then(|notifications| notifications.mute(app, *muted)))
        }
    })
}

fn open(daemon: &Daemon, id: u32, output: Option<String>) -> Answer {
    let clicked = match daemon.module::<Notifications>().and_then(|notifications| notifications.click(id)) {
        Ok(clicked) => clicked,
        Err(e) => return Answer::Now(Outcome::Error(e)),
    };
    let (Some(app), Ok(launcher)) = (clicked.app_id, daemon.module::<crate::launcher::Launcher>()) else {
        return Answer::Now(done(Ok(())));
    };
    if !clicked.heard {
        return crate::launcher::run(daemon, nova_launcher_api::PROVIDER_APPS, &app, output, false);
    }
    let (Some(ids), Some(compositor)) = (launcher.window_ids(&app), daemon.compositor.clone()) else {
        return Answer::Now(done(Ok(())));
    };
    let logger = Arc::clone(&daemon.logger);
    Answer::later(move || {
        nova_launcher::focus_shown(
            nova_launcher::AppWindows { id: &app, ids: &ids },
            &compositor,
            output.as_deref(),
            nova_launcher::SHOWN_WINDOW_WAIT,
            &logger,
        );
        done(Ok(()))
    })
}

impl nova_modules::Module for Notifications {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const NEEDS_APPS: bool = true;
    const TOPIC: Option<nova_carrier::Topic> = Some(nova_notifications_api::TOPIC);
    type Sources = Sources;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> Sources {
        Sources::new(&env.args.state_dir, Arc::clone(env.icons))
    }

    fn config(_: &crate::config::Config) {}

    fn start(
        ctx: &crate::modules::Context,
        sources: &Sources,
        (): &(),
        _: &[nova_daemon_info::FeatureState],
    ) -> Arc<Self> {
        let core = nova_notifications::Notifications::start(
            sources.clone(),
            Arc::clone(&ctx.apps),
            Arc::clone(&ctx.hub),
            Arc::clone(&ctx.logger),
        );
        Arc::new(Self(core))
    }

    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn stop(&self) {
        self.0.stop();
    }
}
