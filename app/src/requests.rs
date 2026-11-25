use crate::Daemon;
use nova_bar_api::Panel;
use nova_base::lock;
use nova_carrier::{Outcome, Topic};
use nova_daemon_info::{Command, Event, topics};
use nova_hub::ClientId;
use nova_socket::{Answer, Caller, Router, done, value};

pub fn router() -> Router<Daemon> {
    Router::new()
        .route::<Own>(
            |daemon, caller, Own(command)| Connection::new(daemon, caller).answer(command),
            |daemon, caller, Own(command)| Connection::new(daemon, caller).follow_up(command),
        )
        .route::<nova_ai_usage_api::Command>(crate::ai_usage::answer, |_, _, _| {})
        .route::<nova_bar_api::Command>(
            |daemon, caller, nova_bar_api::Command::Toggle { panel }| Connection::new(daemon, caller).toggle(*panel),
            |_, _, _| {},
        )
        .route::<nova_bluetooth_api::Command>(crate::bluetooth::answer, |_, _, _| {})
        .route::<nova_launcher_api::Command>(crate::launcher::answer, |_, _, _| {})
        .route::<nova_lock_api::Command>(crate::session_lock::answer, |_, _, _| {})
        .route::<nova_media_api::Command>(crate::media::answer, |_, _, _| {})
        .route::<nova_network_api::Command>(crate::network::answer, |_, _, _| {})
        .route::<nova_notifications_api::Command>(crate::notifications::answer, |_, _, _| {})
        .route::<nova_power_api::Command>(crate::power::answer, |_, _, _| {})
        .route::<nova_recording_api::Command>(crate::recording::answer, |_, _, _| {})
        .route::<nova_resources_api::Command>(crate::resources::answer, |_, _, _| {})
        .route::<nova_screenshot_api::Command>(crate::screenshot::answer, |_, _, _| {})
        .route::<nova_sound_api::Command>(crate::sound::answer, |_, _, _| {})
        .route::<nova_tray_api::Command>(crate::tray::answer, |_, _, _| {})
        .route::<nova_volume_osd_api::Command>(
            |daemon, caller, nova_volume_osd_api::Command::Volume { key }| Connection::new(daemon, caller).volume(*key),
            |_, _, _| {},
        )
        .route::<nova_wallpaper_api::Command>(crate::wallpaper::answer, |_, _, _| {})
        .route::<nova_workspaces_api::Command>(
            |daemon, caller, command| Connection::new(daemon, caller).workspaces(command),
            |_, _, _| {},
        )
}

struct Own(Command);

impl<'de> serde::Deserialize<'de> for Own {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let command = Command::deserialize(deserializer)?;
        if let Command::Subscribe { topics } = &command {
            for name in topics {
                crate::topics::named::<D::Error>(name)?;
            }
        }
        Ok(Self(command))
    }
}

#[cfg(test)]
const ANSWERED_HERE: &[nova_carrier::Topic] = &[
    topics::FEATURES,
    topics::LOGS,
    nova_theme_info::TOPIC,
    nova_desktop_info::TOPIC,
    nova_host_info::TOPIC,
    nova_bar_api::TOPIC,
    nova_volume_osd_api::TOPIC,
];

struct Connection<'a> {
    daemon: &'a Daemon,
    caller: &'a Caller,
    id: ClientId,
}

impl<'a> Connection<'a> {
    fn new(daemon: &'a Daemon, caller: &'a Caller) -> Self {
        Self { daemon, caller, id: caller.id() }
    }

    fn answer(&self, command: &Command) -> Answer {
        let outcome = match command {
            Command::Ping => Outcome::Result("pong".into()),
            Command::ListFeatures => value(Ok(&lock(&self.daemon.settings).features)),
            Command::Status => value(Ok(self.daemon.status())),
            Command::Subscribe { .. } | Command::Quit => done(Ok(())),
        };
        Answer::Now(outcome)
    }

    fn workspaces(&self, command: &nova_workspaces_api::Command) -> Answer {
        use nova_workspaces_api::Command;
        let compositor = self.compositor();
        Answer::Now(done(match command {
            Command::FocusWorkspace { id } => compositor.and_then(|compositor| compositor.focus_workspace(*id)),
            Command::StepWorkspace { output, step } => {
                compositor.and_then(|compositor| compositor.step_workspace(output, *step))
            }
        }))
    }

    fn compositor(&self) -> Result<&nova_compositor::Compositor, String> {
        self.daemon.compositor.as_deref().ok_or_else(|| "novad doesn't know this compositor".into())
    }

    fn require_feature(&self, id: &str) -> Result<(), String> {
        match self.daemon.feature_ready(id) {
            true => Ok(()),
            false => Err(nova_modules::not_ready(id)),
        }
    }

    fn toggle(&self, panel: Panel) -> Answer {
        if let Err(e) = self.require_feature(panel.name()) {
            return Answer::Now(Outcome::Error(e));
        }
        let output = self.daemon.compositor.as_ref().and_then(|compositor| compositor.focused_output());
        Answer::Now(match self.daemon.hub.publish(&nova_bar_api::Event::Toggle { panel, output }) {
            0 => Outcome::Error("the UI isn't connected to novad, so there's nothing to toggle".into()),
            _ => Outcome::Result(serde_json::Value::Null),
        })
    }

    fn volume(&self, key: nova_volume_osd_api::VolumeKey) -> Answer {
        self.daemon.hub.publish(&nova_volume_osd_api::Event::Volume { key });
        crate::sound::press(self.daemon, key)
    }

    fn follow_up(&self, command: Command) {
        match command {
            Command::Subscribe { topics: names } => {
                for topic in names.iter().filter_map(|name| crate::topics::named::<serde_json::Error>(name).ok()) {
                    self.subscribe(topic);
                }
            }
            Command::Quit => {
                self.flush();
                self.daemon.quit();
            }
            Command::Ping | Command::ListFeatures | Command::Status => {}
        }
    }

    fn subscribe(&self, topic: Topic) {
        if topic == topics::LOGS {
            self.daemon.logger.subscribe(self.id);
            return;
        }
        let settings = lock(&self.daemon.settings);
        self.daemon.hub.subscribe(self.id, topic);
        match topic {
            topics::FEATURES => self.send(Event::Features { features: settings.features.clone() }),
            nova_theme_info::TOPIC => self.send(nova_theme_info::Event::Theme(settings.theme.clone())),
            nova_desktop_info::TOPIC => {
                if let Some(compositor) = &self.daemon.compositor {
                    compositor.subscribe(self.id);
                }
            }
            nova_host_info::TOPIC => self.send(nova_host_info::Event::Host(settings.host())),
            nova_bar_api::TOPIC | nova_volume_osd_api::TOPIC => {}
            _ => settings.modules.send_state(topic, self.id),
        }
    }

    fn send(&self, message: impl serde::Serialize) {
        self.caller.send(message);
    }

    fn flush(&self) {
        self.caller.flush();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn each_topic_has_one_owner() {
        let modules: Vec<_> = crate::modules::Running::TOPICS.iter().flatten().copied().collect();
        for topic in crate::topics::TOPICS {
            let owners = modules.iter().filter(|module| *module == topic).count()
                + usize::from(super::ANSWERED_HERE.contains(topic));
            assert_eq!(owners, 1, "{topic:?} has {owners} owners");
        }
        assert!(modules.iter().all(|topic| crate::topics::TOPICS.contains(topic)));
    }
}
