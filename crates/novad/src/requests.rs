//! What novad answers on the socket: one route per message type in
//! [`router`]. `nova_protocol::Command` is the shared list the feature moves
//! take apart (IMP-368); a feature that owns its commands (its api crate's
//! `Command`) has its own route, which calls into its module. The socket
//! itself is `nova-socket`'s.

use crate::hub::ClientId;
use crate::{Daemon, lock};
use nova_protocol::{Command, Event, Outcome, Panel, VolumeKey, topics};
use nova_socket::{Answer, Caller, Router, done, value};
use std::sync::Arc;

/// The routes of every message novad serves. A moved feature adds its
/// route in its place in the list below (by its feature name), and its
/// commands leave `nova_protocol::Command`.
pub fn router() -> Router<Daemon> {
    Router::new()
        .route::<Shared>(
            |daemon, caller, Shared(command)| Connection::new(daemon, caller).answer(command),
            |daemon, caller, Shared(command)| Connection::new(daemon, caller).follow_up(command),
        )
        .route::<nova_ai_usage_api::Command>(crate::ai_usage::answer, |_, _, _| {})
        .route::<nova_bluetooth_api::Command>(crate::bluetooth::answer, |_, _, _| {})
        .route::<nova_launcher_api::Command>(crate::launcher::answer, |_, _, _| {})
        .route::<nova_lock_api::Command>(crate::session_lock::answer, |_, _, _| {})
        .route::<nova_media_api::Command>(crate::media::answer, |_, _, _| {})
        .route::<nova_network_api::Command>(crate::network::answer, |_, _, _| {})
        .route::<nova_notifications_api::Command>(crate::notifications::answer, |_, _, _| {})
        .route::<nova_power_api::Command>(crate::power::answer, |_, _, _| {})
        .route::<nova_resources_api::Command>(crate::resources::answer, |_, _, _| {})
        .route::<nova_screenshot_api::Command>(crate::screenshot::answer, |_, _, _| {})
        .route::<nova_tray_api::Command>(crate::tray::answer, |_, _, _| {})
        .route::<nova_wallpaper_api::Command>(crate::wallpaper::answer, |_, _, _| {})
        .listed_by(protocol_js_position)
}

/// The UI's protocol file. It is generated from these routes, so it names
/// every command, and its order stays put as features move theirs out of
/// `nova_protocol::Command` (`protocol_js`).
const PROTOCOL_JS: &str = include_str!("../../../ui/daemon/protocol.js");

/// Where protocol.js names the command `name`, so an unknown command's
/// error lists the names in one fixed order, whatever order the routes
/// were added in.
fn protocol_js_position(name: &str) -> usize {
    PROTOCOL_JS
        .lines()
        .position(|line| line.strip_prefix("var CMD_").and_then(|rest| rest.split('"').nth(1)) == Some(name))
        .unwrap_or(usize::MAX)
}

/// A shared command, read with its `subscribe` topics checked against the
/// ones novad serves ([`crate::topics`]): an unknown one makes the request
/// bad, as an unknown variant of a topic enum would.
struct Shared(Command);

impl<'de> serde::Deserialize<'de> for Shared {
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
const ANSWERED_HERE: &[nova_protocol::Topic] =
    &[topics::FEATURES, topics::LOGS, topics::THEME, topics::COMPOSITOR, topics::HOST, topics::TOGGLE, topics::VOLUME];

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

    /// The answer to `command`, at once or as work for another thread: the
    /// requests that wait on logind, the compositor, an app starting or a
    /// screen capture. Their quick checks run here, at once.
    fn answer(&self, command: &Command) -> Answer {
        let outcome = match command {
            Command::Ping => Outcome::Result("pong".into()),
            Command::ListFeatures => value(Ok(&lock(&self.daemon.settings).features)),
            Command::Status => value(Ok(self.daemon.status())),
            Command::Subscribe { .. } | Command::Quit => done(Ok(())),
            Command::FocusWorkspace { id } => {
                done(self.compositor().and_then(|compositor| compositor.focus_workspace(*id)))
            }
            Command::StepWorkspace { output, step } => {
                done(self.compositor().and_then(|compositor| compositor.step_workspace(output, *step)))
            }
            Command::Toggle { panel } => self.toggle(*panel),
            Command::Volume { key } => self.volume(*key),
            Command::Record => {
                done(self.require_feature(crate::recording::FEATURE_ID).and_then(|()| self.recordings().record()))
            }
            Command::StartRecording { output, region, audio } => {
                return self.start_recording(output.clone(), *region, *audio);
            }
            // Even with the feature off, so a reload can't leave one running.
            Command::StopRecording => done(self.recordings().stop()),
            Command::PauseRecording { paused } => done(self.recordings().pause(*paused)),
            Command::RecordingAction { id, action } => done(self.recordings().act(*id, *action)),
            Command::OpenLog => {
                done(self.require_feature(crate::recording::FEATURE_ID).and_then(|()| self.recordings().open_log()))
            }
        };
        Answer::Now(outcome)
    }

    // Kept for novad's life, whatever the settings.

    fn recordings(&self) -> Arc<crate::recording::Recordings> {
        Arc::clone(&self.daemon.kept.recordings)
    }

    fn compositor(&self) -> Result<&crate::compositor::Compositor, String> {
        self.daemon.compositor.as_deref().ok_or_else(|| "novad doesn't know this compositor".into())
    }

    /// An error saying the feature `id` isn't ready, when it isn't.
    fn require_feature(&self, id: &str) -> Result<(), String> {
        match self.daemon.feature_ready(id) {
            true => Ok(()),
            false => Err(crate::features::not_ready(id)),
        }
    }

    /// Starts the recording the UI's picker asked for.
    fn start_recording(
        &self,
        output: String,
        region: Option<nova_protocol::LayoutRect>,
        audio: nova_protocol::RecordAudio,
    ) -> Answer {
        if let Err(e) = self.require_feature(crate::recording::FEATURE_ID) {
            return Answer::Now(Outcome::Error(e));
        }
        let recordings = self.recordings();
        let folder = lock(&self.daemon.settings).recording_folder();
        Answer::later(move || done(folder.and_then(|folder| recordings.start(&output, region, audio, &folder))))
    }

    /// Tells the UI to open or close `panel` on the focused output. The UI
    /// knows which screen has it open, so it decides which.
    fn toggle(&self, panel: Panel) -> Outcome {
        if let Err(e) = self.require_feature(panel.name()) {
            return Outcome::Error(e);
        }
        let output = self.daemon.compositor.as_ref().and_then(|compositor| compositor.focused_output());
        match self.daemon.hub.publish(&Event::Toggle { panel, output }) {
            0 => Outcome::Error("the UI isn't connected to novad, so there's nothing to toggle".into()),
            _ => Outcome::Result(serde_json::Value::Null),
        }
    }

    /// Passes a volume key to the UI, which owns audio (PipeWire through
    /// Quickshell) and the capsule. The keys change the volume while the UI
    /// runs, whichever features draw it (the capsule, quick settings).
    fn volume(&self, key: VolumeKey) -> Outcome {
        match self.daemon.hub.publish(&Event::Volume { key }) {
            0 => Outcome::Error("the UI isn't connected to novad, so nothing can change the volume".into()),
            _ => Outcome::Result(serde_json::Value::Null),
        }
    }

    /// Work done after the reply is queued.
    fn follow_up(&self, command: Command) {
        match command {
            Command::Subscribe { topics: names } => {
                // The names were checked as the request was read (`Shared`).
                for topic in names.iter().filter_map(|name| crate::topics::named::<serde_json::Error>(name).ok()) {
                    if topic == topics::LOGS {
                        self.daemon.logger.subscribe(self.id);
                        continue;
                    }
                    // Subscribed and sent the current state under the
                    // settings lock, so a reload's event can't come first.
                    let settings = lock(&self.daemon.settings);
                    self.daemon.hub.subscribe(self.id, topic);
                    match topic {
                        topics::FEATURES => self.send(Event::Features { features: settings.features.clone() }),
                        topics::THEME => self.send(Event::Theme(settings.theme.clone())),
                        // Sent once the compositor has said what it shows.
                        topics::COMPOSITOR => {
                            if let Some(compositor) = &self.daemon.compositor {
                                compositor.subscribe(self.id);
                            }
                        }
                        // Facts about the machine, there whatever the features.
                        topics::HOST => self.send(Event::Host(settings.host())),
                        // Only ever sent as `nova toggle` or `nova volume` asks.
                        topics::TOGGLE | topics::VOLUME => {}
                        // A module's (`Module::TOPIC`, checked by
                        // `each_topic_has_one_owner`): without its feature
                        // there is nothing to send until a reload turns it on.
                        _ => settings.modules.send_state(topic, self.id),
                    }
                }
            }
            Command::Quit => {
                self.flush();
                self.daemon.quit();
            }
            Command::Ping
            | Command::ListFeatures
            | Command::Status
            | Command::FocusWorkspace { .. }
            | Command::StepWorkspace { .. }
            | Command::Toggle { .. }
            | Command::Record
            | Command::StartRecording { .. }
            | Command::StopRecording
            | Command::PauseRecording { .. }
            | Command::RecordingAction { .. }
            | Command::OpenLog
            | Command::Volume { .. } => {}
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
