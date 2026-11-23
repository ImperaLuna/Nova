//! What novad answers on the socket: one route per message type in
//! [`router`]. novad's own commands (`nova_daemon_info::Command`) and the
//! ones it answers without a module (workspaces, `nova toggle`, `nova
//! volume`) are answered here; every other feature's route calls into its
//! module. The socket itself is `nova-socket`'s.

use crate::Daemon;
use nova_bar_api::Panel;
use nova_base::lock;
use nova_carrier::{Outcome, Topic};
use nova_daemon_info::{Command, Event, UiProgram, topics};
use nova_hub::ClientId;
use nova_socket::{Answer, Caller, Router, done, value};

/// The routes of every message novad serves: novad's own first, then each
/// feature's, by its feature name.
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
        .listed_by(protocol_js_position)
}

/// The UI's protocol file. It is generated from these routes, so it names
/// every command, and its order stays put as routes change (`protocol_js`).
const PROTOCOL_JS: &str = include_str!("../../ui/daemon/protocol.js");

/// Where protocol.js names the command `name`, so an unknown command's
/// error lists the names in one fixed order, whatever order the routes
/// were added in.
fn protocol_js_position(name: &str) -> usize {
    PROTOCOL_JS
        .lines()
        .position(|line| line.strip_prefix("var CMD_").and_then(|rest| rest.split('"').nth(1)) == Some(name))
        .unwrap_or(usize::MAX)
}

/// One of novad's own commands, read with its `subscribe` topics checked
/// against the ones novad serves ([`crate::topics`]): an unknown one makes
/// the request bad, as an unknown variant of a topic enum would.
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

/// The topics `follow_up` answers itself on subscribe; every other one in
/// [`crate::topics::TOPICS`] is a module's.
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

/// A request from the client `caller`.
struct Connection<'a> {
    daemon: &'a Daemon,
    caller: &'a Caller,
    id: ClientId,
}

impl<'a> Connection<'a> {
    fn new(daemon: &'a Daemon, caller: &'a Caller) -> Self {
        Self { daemon, caller, id: caller.id() }
    }

    /// The answer to `command`.
    fn answer(&self, command: &Command) -> Answer {
        let outcome = match command {
            Command::Ping => Outcome::Result("pong".into()),
            Command::ListFeatures => value(Ok(&lock(&self.daemon.settings).features)),
            Command::Status => value(Ok(self.daemon.status())),
            Command::Subscribe { .. } | Command::Quit => done(Ok(())),
        };
        Answer::Now(outcome)
    }

    /// Asks the compositor to show a workspace.
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

    /// An error saying the feature `id` isn't ready, when it isn't.
    fn require_feature(&self, id: &str) -> Result<(), String> {
        match self.daemon.feature_ready(id) {
            true => Ok(()),
            false => Err(nova_modules::not_ready(id)),
        }
    }

    /// Tells the UI to open or close `panel` on the focused output. The UI
    /// knows which screen has it open, so it decides which.
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

    /// Passes a volume key to the UI, which shows the capsule. Quickshell's
    /// shell changes the volume itself (PipeWire through Quickshell), so
    /// the key does nothing without it; for the GPUI UI novad changes it,
    /// through the sound feature, whether or not a UI is there to show it.
    fn volume(&self, key: nova_volume_osd_api::VolumeKey) -> Answer {
        let shown = self.daemon.hub.publish(&nova_volume_osd_api::Event::Volume { key });
        if lock(&self.daemon.settings).ui_in_use() == UiProgram::Gpui {
            return crate::sound::press(self.daemon, key);
        }
        Answer::Now(match shown {
            0 => Outcome::Error("the UI isn't connected to novad, so nothing can change the volume".into()),
            _ => Outcome::Result(serde_json::Value::Null),
        })
    }

    /// Work done after the reply is queued.
    fn follow_up(&self, command: Command) {
        match command {
            Command::Subscribe { topics: names } => {
                // The names were checked as the request was read (`Own`).
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

    /// Subscribes the client to `topic` and sends what it shows now.
    fn subscribe(&self, topic: Topic) {
        if topic == topics::LOGS {
            self.daemon.logger.subscribe(self.id);
            return;
        }
        // Subscribed and sent the current state under the settings lock, so a
        // reload's event can't come first.
        let settings = lock(&self.daemon.settings);
        self.daemon.hub.subscribe(self.id, topic);
        match topic {
            topics::FEATURES => self.send(Event::Features { features: settings.features.clone() }),
            nova_theme_info::TOPIC => self.send(nova_theme_info::Event::Theme(settings.theme.clone())),
            // Sent once the compositor has said what it shows.
            nova_desktop_info::TOPIC => {
                if let Some(compositor) = &self.daemon.compositor {
                    compositor.subscribe(self.id);
                }
            }
            // Facts about the machine, there whatever the features.
            nova_host_info::TOPIC => self.send(nova_host_info::Event::Host(settings.host())),
            // Only ever sent as `nova toggle` or `nova volume` asks.
            nova_bar_api::TOPIC | nova_volume_osd_api::TOPIC => {}
            // A module's (`Module::TOPIC`, checked by
            // `each_topic_has_one_owner`): without its feature
            // there is nothing to send until a reload turns it on.
            _ => settings.modules.send_state(topic, self.id),
        }
    }

    fn send(&self, message: impl serde::Serialize) {
        self.caller.send(message);
    }

    /// Waits until everything queued so far is written.
    fn flush(&self) {
        self.caller.flush();
    }
}

#[cfg(test)]
mod tests {
    /// Each topic is answered on subscribe by `follow_up` itself or by
    /// exactly one module, and every module's topic is one novad serves.
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

    /// Every command the UI's protocol file names has exactly one route, and
    /// no route takes a command the file lacks. (A name two routes claim
    /// already stops the router being built.)
    #[test]
    fn every_command_has_one_route() {
        let js = super::PROTOCOL_JS;
        let mut named: Vec<&str> =
            js.lines().filter_map(|line| line.strip_prefix("var CMD_")?.split('"').nth(1)).collect();
        let mut routed = super::router().names().to_vec();
        assert!(!named.is_empty(), "protocol.js names no commands");
        named.sort_unstable();
        routed.sort_unstable();
        assert_eq!(routed, named);
    }
}
