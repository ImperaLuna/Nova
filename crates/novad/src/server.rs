//! The socket: accepts clients, answers their requests and feeds them the
//! events they subscribed to.

use crate::hub::{ClientId, ClientQueue, Outgoing};
use crate::{Daemon, lock};
use nova_protocol::{
    Capability, CaptureMode, Command, Event, Outcome, Panel, PowerAction, PowerMenu, Response, Topic, VolumeKey, encode,
};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

/// Any local process can connect: longer request lines close the connection.
pub const MAX_REQUEST_BYTES: u64 = 64 * 1024;
/// How long a write may make no progress before the client counts as stalled
/// and is disconnected. Only a client that stops reading with its socket
/// buffer full gets there; the UI reconnects on its own.
pub const WRITE_TIMEOUT: Duration = if cfg!(test) { Duration::from_millis(300) } else { Duration::from_secs(10) };
const SOCKET_DIR_MODE: u32 = 0o700;
/// A request's answer, worked out off the connection's thread.
type SlowWork = Box<dyn FnOnce() -> Outcome + Send>;

const LOG_SOURCE: &str = "server";
/// How long the launcher waits for the window of an app it reopened from
/// the tray, to focus it, and how often it looks.
const TRAY_WINDOW_WAIT: Duration = if cfg!(test) { Duration::from_millis(300) } else { Duration::from_secs(3) };
const TRAY_WINDOW_RETRY: Duration = Duration::from_millis(100);

pub fn bind(socket: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = socket.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(SOCKET_DIR_MODE).create(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(SOCKET_DIR_MODE))?;
    }
    if UnixStream::connect(socket).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!("another novad is already listening on {}", socket.display()),
        ));
    }
    // A socket file left by a previous run would make bind fail.
    match std::fs::remove_file(socket) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    UnixListener::bind(socket)
}

pub fn serve(listener: UnixListener, daemon: Arc<Daemon>) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let daemon = Arc::clone(&daemon);
                std::thread::spawn(move || {
                    if let Err(e) = Connection::open(stream, &daemon).and_then(|c| c.run()) {
                        daemon.logger.warn(LOG_SOURCE, &format!("client error: {e}"));
                    }
                });
            }
            Err(e) => daemon.logger.warn(LOG_SOURCE, &format!("accept failed: {e}")),
        }
    }
}

struct Connection<'a> {
    daemon: &'a Daemon,
    id: ClientId,
    queue: ClientQueue,
    reader: BufReader<UnixStream>,
    // The connecting process, from the kernel (SO_PEERCRED).
    peer_pid: Option<u32>,
}

impl<'a> Connection<'a> {
    fn open(stream: UnixStream, daemon: &'a Daemon) -> std::io::Result<Self> {
        let writer = stream.try_clone()?;
        writer.set_write_timeout(Some(WRITE_TIMEOUT))?;
        let (queue, outgoing) = ClientQueue::new(&stream)?;
        let logger = Arc::clone(&daemon.logger);
        std::thread::spawn(move || write_all_queued(writer, outgoing, &logger));
        let id = daemon.hub.register(queue.clone());
        let peer_pid = peer_pid(&stream);
        let connection = Self { daemon, id, queue, reader: BufReader::new(stream), peer_pid };
        connection.send(Event::Hello { daemon: env!("CARGO_PKG_VERSION").into() });
        Ok(connection)
    }

    fn run(mut self) -> std::io::Result<()> {
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = (&mut self.reader).take(MAX_REQUEST_BYTES + 1).read_until(b'\n', &mut line)?;
            if read == 0 {
                return Ok(());
            }
            if line.last() != Some(&b'\n') && read as u64 > MAX_REQUEST_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("request over {MAX_REQUEST_BYTES} bytes, closing"),
                ));
            }
            let text = String::from_utf8_lossy(&line);
            if !text.trim().is_empty() {
                self.handle(text.trim());
            }
        }
    }

    fn handle(&self, line: &str) {
        let request = match nova_protocol::decode_request(line) {
            Ok(request) => request,
            Err(e) => {
                self.daemon.logger.warn(LOG_SOURCE, &format!("ignoring bad request: {e}"));
                if let Some(id) = request_id(line) {
                    self.send(Response { id, outcome: Outcome::Error(format!("bad request: {e}")) });
                }
                return;
            }
        };
        // Asking logind can take seconds; answering on its own thread keeps
        // this connection reading, so the UI's lock reports and other
        // requests aren't stuck behind it.
        if let Some(work) = self.slow_work(&request.command) {
            self.answer_later(request.id, work);
            self.follow_up(request.command);
            return;
        }
        // Answered even without an id: some commands act as they answer.
        let outcome = self.answer(&request.command);
        if let Some(id) = request.id {
            self.send(Response { id, outcome });
        }
        self.follow_up(request.command);
    }

    fn answer(&self, command: &Command) -> Outcome {
        let value = match command {
            Command::Ping => Ok("pong".into()),
            Command::ListFeatures => serde_json::to_value(&lock(&self.daemon.settings).features),
            Command::Status => serde_json::to_value(self.daemon.status()),
            Command::Subscribe { .. } | Command::Quit => Ok(serde_json::Value::Null),
            Command::RefreshAiUsage => {
                return self.with_ai_usage(|usage| {
                    usage.refresh();
                    Ok(())
                });
            }
            Command::ShowAiUsage { provider } => {
                return self.with_ai_usage(|usage| usage.show(*provider));
            }
            Command::ReportClaudeUsage { limits } => {
                return self.with_ai_usage(|usage| usage.report_claude(limits.clone()));
            }
            Command::FocusWorkspace { id } => {
                return self.with_compositor(|compositor| compositor.focus_workspace(*id));
            }
            Command::StepWorkspace { output, step } => {
                return self.with_compositor(|compositor| compositor.step_workspace(output, *step));
            }
            Command::Toggle { panel } => return self.toggle(*panel),
            Command::Volume { key } => return self.volume(*key),
            Command::Lock => return self.lock_screen(),
            Command::ReportLocked => {
                if let Err(e) = self.sent_by_the_ui() {
                    return Outcome::Error(e);
                }
                self.daemon.session_lock.secured();
                Ok(serde_json::Value::Null)
            }
            // Even with the feature off: the screen was locked while it was on.
            Command::ReportUnlocked => {
                if let Err(e) = self.sent_by_the_ui() {
                    return Outcome::Error(e);
                }
                self.daemon.session_lock.unlocked(&self.daemon.hub);
                Ok(serde_json::Value::Null)
            }
            Command::WatchResources { watching } => return self.watch_resources(*watching),
            Command::EndApp { key } => return self.end_app(key),
            Command::PowerOptions => return self.power_options()(),
            Command::Power { action } => return self.power(*action),
            Command::Screenshot { mode } => return self.screenshot(*mode)(),
            Command::SaveCapture { output, rect } => return self.save_capture(output.clone(), *rect)(),
            // Even with the feature off, so a reload can't strand the picker.
            Command::CancelCapture => {
                self.daemon.screenshots.cancel();
                Ok(serde_json::Value::Null)
            }
            Command::ScreenshotAction { id, action } => {
                return match self.daemon.screenshots.act(*id, *action) {
                    Ok(()) => Outcome::Result(serde_json::Value::Null),
                    Err(e) => Outcome::Error(e),
                };
            }
            Command::Record => {
                let recordings = &self.daemon.recordings;
                return outcome(self.require_feature(crate::recording::FEATURE_ID).and_then(|()| recordings.record()));
            }
            Command::StartRecording { output, region, audio } => {
                return self.start_recording(output.clone(), *region, *audio)();
            }
            // Even with the feature off, so a reload can't leave one running.
            Command::StopRecording => return outcome(self.daemon.recordings.stop()),
            Command::PauseRecording { paused } => return outcome(self.daemon.recordings.pause(*paused)),
            Command::RecordingAction { id, action } => return outcome(self.daemon.recordings.act(*id, *action)),
            Command::SearchApps { query } => {
                let launcher = lock(&self.daemon.settings).launcher.clone();
                return match launcher.map(|launcher| launcher.search(query)) {
                    Some(Ok(found)) => match serde_json::to_value(found) {
                        Ok(value) => Outcome::Result(value),
                        Err(e) => Outcome::Error(e.to_string()),
                    },
                    Some(Err(e)) => Outcome::Error(e),
                    None => Outcome::Error(format!("the {} feature is off", crate::launcher::FEATURE_ID)),
                };
            }
            // Answered on its own thread (`slow_work`).
            Command::LaunchApp { id, output, new_window } => return self.launch_app(id, output.clone(), *new_window)(),
            Command::PinApp { id, pinned } => return self.with_launcher(|launcher| launcher.pin(id, *pinned)),
            Command::HideTrayApp { id, hidden } => {
                let tray = lock(&self.daemon.settings).tray.clone();
                return match tray.map(|tray| tray.hide(id, *hidden)) {
                    Some(Ok(())) => Outcome::Result(serde_json::Value::Null),
                    Some(Err(e)) => Outcome::Error(e),
                    None => Outcome::Error(format!("the {} feature is off", crate::tray::FEATURE_ID)),
                };
            }
            Command::NotificationAction { id, action } => {
                return self.with_notifications(|notifications| notifications.action(*id, action.as_deref()));
            }
            Command::DismissNotification { id } => {
                return self.with_notifications(|notifications| {
                    notifications.dismiss(*id);
                    Ok(())
                });
            }
            Command::ClearNotifications { app } => {
                return self.with_notifications(|notifications| {
                    notifications.clear(app.as_deref());
                    Ok(())
                });
            }
            Command::ReadNotifications => {
                return self.with_notifications(|notifications| {
                    notifications.read();
                    Ok(())
                });
            }
            Command::SetDoNotDisturb { on } => {
                return self.with_notifications(|notifications| {
                    notifications.set_do_not_disturb(*on);
                    Ok(())
                });
            }
            Command::MuteNotificationApp { app, muted } => {
                return self.with_notifications(|notifications| notifications.mute(app, *muted));
            }
        };
        match value {
            Ok(value) => Outcome::Result(value),
            Err(e) => Outcome::Error(e.to_string()),
        }
    }

    fn with_ai_usage(&self, act: impl FnOnce(&crate::ai_usage::Usage) -> Result<(), String>) -> Outcome {
        // Out of the lock: acting may take a while, and a reload may stop it
        // meanwhile (it then says the feature is off).
        let usage = lock(&self.daemon.settings).ai_usage.clone();
        match usage {
            Some(usage) => match act(&usage) {
                Ok(()) => Outcome::Result(serde_json::Value::Null),
                Err(e) => Outcome::Error(e),
            },
            None => Outcome::Error(format!("the {} feature is off", crate::ai_usage::FEATURE_ID)),
        }
    }

    fn with_launcher(&self, act: impl FnOnce(&crate::launcher::Launcher) -> Result<(), String>) -> Outcome {
        // Out of the lock, like `with_ai_usage`.
        let launcher = lock(&self.daemon.settings).launcher.clone();
        match launcher {
            Some(launcher) => match act(&launcher) {
                Ok(()) => Outcome::Result(serde_json::Value::Null),
                Err(e) => Outcome::Error(e),
            },
            None => Outcome::Error(format!("the {} feature is off", crate::launcher::FEATURE_ID)),
        }
    }

    fn with_notifications(
        &self,
        act: impl FnOnce(&crate::notifications::Notifications) -> Result<(), String>,
    ) -> Outcome {
        // Out of the lock, like `with_ai_usage`.
        let notifications = lock(&self.daemon.settings).notifications.clone();
        match notifications {
            Some(notifications) => match act(&notifications) {
                Ok(()) => Outcome::Result(serde_json::Value::Null),
                Err(e) => Outcome::Error(e),
            },
            None => Outcome::Error(format!("the {} feature is off", crate::notifications::FEATURE_ID)),
        }
    }

    fn with_compositor(&self, act: impl FnOnce(&crate::compositor::Compositor) -> Result<(), String>) -> Outcome {
        match &self.daemon.compositor {
            Some(compositor) => match act(compositor) {
                Ok(()) => Outcome::Result(serde_json::Value::Null),
                Err(e) => Outcome::Error(e),
            },
            None => Outcome::Error("novad doesn't know this compositor".into()),
        }
    }

    fn watch_resources(&self, watching: bool) -> Outcome {
        let resources = lock(&self.daemon.settings).resources.clone();
        match resources {
            Some(resources) => {
                resources.watch(self.id, watching);
                Outcome::Result(serde_json::Value::Null)
            }
            None => Outcome::Error(format!("the {} feature is off", crate::resources::FEATURE_ID)),
        }
    }

    fn end_app(&self, key: &str) -> Outcome {
        let resources = lock(&self.daemon.settings).resources.clone();
        match resources.map(|resources| resources.end_app(key)) {
            Some(Ok(())) => Outcome::Result(serde_json::Value::Null),
            Some(Err(e)) => Outcome::Error(e),
            None => Outcome::Error(format!("the {} feature is off", crate::resources::FEATURE_ID)),
        }
    }

    /// Tells the UI to open or close `panel` on the focused output. The UI
    /// knows which screen has it open, so it decides which.
    /// Lock reports are the UI's alone: from anyone else, `report_unlocked`
    /// would leave a UI that restarts with no lock screen to unlock. When
    /// novad runs no UI (`--no-ui`: tests, or a UI run by hand), it can't
    /// tell which client is the UI and takes them from any.
    fn sent_by_the_ui(&self) -> Result<(), String> {
        let Some(ui) = &self.daemon.ui else { return Ok(()) };
        match ui.status().pid {
            Some(pid) if self.peer_pid == Some(pid) => Ok(()),
            _ => Err("only the UI novad runs can report the lock screen's state".into()),
        }
    }

    /// An error saying the feature `id` isn't ready, when it isn't.
    fn require_feature(&self, id: &str) -> Result<(), String> {
        match self.daemon.feature_ready(id) {
            true => Ok(()),
            false => Err(format!("the {id} feature isn't ready (see `nova status`)")),
        }
    }

    fn lock_screen(&self) -> Outcome {
        let locked = self
            .require_feature(crate::session_lock::FEATURE_ID)
            .and_then(|()| self.daemon.session_lock.lock(&self.daemon.hub));
        match locked {
            Ok(()) => Outcome::Result(serde_json::Value::Null),
            Err(e) => Outcome::Error(e),
        }
    }

    /// The requests that wait on logind, the compositor, an app starting
    /// or a screen capture, as work to answer them with; the quick checks
    /// before it run here, at once.
    fn slow_work(&self, command: &Command) -> Option<SlowWork> {
        match command {
            Command::LaunchApp { id, output, new_window } => Some(self.launch_app(id, output.clone(), *new_window)),
            Command::PowerOptions => Some(self.power_options()),
            Command::Screenshot { mode } => Some(self.screenshot(*mode)),
            Command::SaveCapture { output, rect } => Some(self.save_capture(output.clone(), *rect)),
            Command::StartRecording { output, region, audio } => {
                Some(self.start_recording(output.clone(), *region, *audio))
            }
            Command::Power {
                action: action @ (PowerAction::Suspend | PowerAction::Reboot | PowerAction::PowerOff),
            } => Some(self.power_logind(*action)),
            _ => None,
        }
    }

    /// Runs `work` on its own thread and sends its answer when it's done.
    fn answer_later(&self, id: Option<u64>, work: SlowWork) {
        let queue = self.queue.clone();
        let logger = Arc::clone(&self.daemon.logger);
        std::thread::spawn(move || {
            let outcome = work();
            let Some(id) = id else { return };
            match encode(Response { id, outcome }) {
                Ok(line) => {
                    queue.push(Outgoing::Line(line));
                }
                Err(e) => logger.error(LOG_SOURCE, &format!("can't encode a message: {e}")),
            }
        });
    }

    fn power_options(&self) -> SlowWork {
        if let Err(e) = self.require_feature(crate::power::FEATURE_ID) {
            return Box::new(move || Outcome::Error(e));
        }
        let lock = self.require_feature(crate::session_lock::FEATURE_ID);
        let log_out = match &self.daemon.compositor {
            Some(compositor) if compositor.can(Capability::LogOut) => Ok(()),
            _ => Err("Not on this compositor".into()),
        };
        let logger = Arc::clone(&self.daemon.logger);
        Box::new(move || {
            let menu = PowerMenu {
                options: crate::power::options(lock, log_out, &logger),
                booted_at: crate::procfs::boot_time(),
            };
            match serde_json::to_value(menu) {
                Ok(value) => Outcome::Result(value),
                Err(e) => Outcome::Error(e.to_string()),
            }
        })
    }

    /// Runs a power menu action. The menu asks over the UI's own
    /// connection, which can't report the lock screen covering the screens
    /// while this waits, so locking here doesn't wait for it.
    fn power(&self, action: PowerAction) -> Outcome {
        if let Err(e) = self.require_feature(crate::power::FEATURE_ID) {
            return Outcome::Error(e);
        }
        match action {
            PowerAction::Lock => {
                self.daemon.logger.info(LOG_SOURCE, &format!("power menu: {action:?}"));
                let locked = self
                    .require_feature(crate::session_lock::FEATURE_ID)
                    .and_then(|()| self.daemon.session_lock.request(&self.daemon.hub));
                match locked {
                    Ok(()) => Outcome::Result(serde_json::Value::Null),
                    Err(e) => Outcome::Error(e),
                }
            }
            PowerAction::LogOut => {
                self.daemon.logger.info(LOG_SOURCE, &format!("power menu: {action:?}"));
                self.with_compositor(crate::compositor::Compositor::log_out)
            }
            PowerAction::Suspend | PowerAction::Reboot | PowerAction::PowerOff => self.power_logind(action)(),
        }
    }

    /// Suspend, restart or shut down through logind; answers with logind's
    /// own result. A suspend locks the screen first through novad's sleep
    /// inhibitor ([`crate::logind`]), as one from anywhere else does.
    fn launch_app(&self, id: &str, output: Option<String>, new_window: bool) -> SlowWork {
        let Some(launcher) = lock(&self.daemon.settings).launcher.clone() else {
            let off = format!("the {} feature is off", crate::launcher::FEATURE_ID);
            return Box::new(move || Outcome::Error(off));
        };
        let compositor = self.daemon.compositor.clone();
        let tray = lock(&self.daemon.settings).tray.clone().filter(|_| !new_window);
        let logger = Arc::clone(&self.daemon.logger);
        let id = id.to_owned();
        Box::new(move || {
            // An open window takes focus where it is; a new one opens on the
            // launcher's screen, which the compositor puts it on once focused.
            // An app that only sits in the tray shows its window from there,
            // as starting it again may just quit (Discord).
            let focused = |ids: &[String]| {
                if let Some(compositor) = &compositor {
                    if !new_window && !ids.is_empty() && compositor.focus_app(ids, output.as_deref())? {
                        return Ok(true);
                    }
                    if let Some(output) = &output {
                        compositor.focus_output(output)?;
                    }
                }
                if !tray.as_ref().map_or(Ok(false), |tray| tray.activate_app(&id))? {
                    return Ok(false);
                }
                // The window comes back without focus on niri.
                if let Some(compositor) = compositor.as_ref().filter(|_| !ids.is_empty()) {
                    let deadline = std::time::Instant::now() + TRAY_WINDOW_WAIT;
                    while !compositor.focus_app(ids, output.as_deref())? {
                        if std::time::Instant::now() >= deadline {
                            logger
                                .info(LOG_SOURCE, &format!("{id}'s window didn't show after its tray icon was asked"));
                            break;
                        }
                        std::thread::sleep(TRAY_WINDOW_RETRY);
                    }
                }
                Ok(true)
            };
            match launcher.launch(&id, focused) {
                Ok(()) => Outcome::Result(serde_json::Value::Null),
                Err(e) => Outcome::Error(e),
            }
        })
    }

    fn power_logind(&self, action: PowerAction) -> SlowWork {
        if let Err(e) = self.require_feature(crate::power::FEATURE_ID) {
            return Box::new(move || Outcome::Error(e));
        }
        self.daemon.logger.info(LOG_SOURCE, &format!("power menu: {action:?}"));
        let logger = Arc::clone(&self.daemon.logger);
        Box::new(move || match crate::power::run_logind(action) {
            Ok(()) => Outcome::Result(serde_json::Value::Null),
            Err(e) => {
                logger.warn(LOG_SOURCE, &format!("{action:?} failed: {e}"));
                Outcome::Error(e)
            }
        })
    }

    /// Opens the region picker on every screen (or closes it), or saves the
    /// focused screen.
    fn screenshot(&self, mode: CaptureMode) -> SlowWork {
        if let Err(e) = self.require_feature(crate::screenshot::FEATURE_ID) {
            return Box::new(move || Outcome::Error(e));
        }
        let screenshots = Arc::clone(&self.daemon.screenshots);
        let compositor = self.daemon.compositor.clone();
        let folder = lock(&self.daemon.settings).screenshot_folder();
        Box::new(move || {
            let taken = match mode {
                CaptureMode::Region => match compositor.and_then(|compositor| compositor.desktop()) {
                    Some(desktop) => {
                        let outputs: Vec<String> = desktop.outputs.into_iter().map(|output| output.name).collect();
                        screenshots.pick_region(&outputs)
                    }
                    None => Err("novad doesn't know this compositor's screens yet".into()),
                },
                CaptureMode::Screen => {
                    let output = compositor.and_then(|compositor| compositor.focused_output());
                    folder.and_then(|folder| screenshots.take_screen(output.as_deref(), &folder))
                }
            };
            match taken {
                Ok(()) => Outcome::Result(serde_json::Value::Null),
                Err(e) => Outcome::Error(e),
            }
        })
    }

    /// Saves the region the user picked.
    fn save_capture(&self, output: String, rect: nova_protocol::PixelRect) -> SlowWork {
        if let Err(e) = self.require_feature(crate::screenshot::FEATURE_ID) {
            return Box::new(move || Outcome::Error(e));
        }
        let screenshots = Arc::clone(&self.daemon.screenshots);
        let folder = lock(&self.daemon.settings).screenshot_folder();
        Box::new(move || match folder.and_then(|folder| screenshots.save_region(&output, rect, &folder)) {
            Ok(()) => Outcome::Result(serde_json::Value::Null),
            Err(e) => Outcome::Error(e),
        })
    }

    /// Starts the recording the UI's picker asked for.
    fn start_recording(
        &self,
        output: String,
        region: Option<nova_protocol::LayoutRect>,
        audio: nova_protocol::RecordAudio,
    ) -> SlowWork {
        if let Err(e) = self.require_feature(crate::recording::FEATURE_ID) {
            return Box::new(move || Outcome::Error(e));
        }
        let recordings = Arc::clone(&self.daemon.recordings);
        let folder = lock(&self.daemon.settings).recording_folder();
        Box::new(move || outcome(folder.and_then(|folder| recordings.start(&output, region, audio, &folder))))
    }

    fn toggle(&self, panel: Panel) -> Outcome {
        if !self.daemon.feature_ready(panel.name()) {
            return Outcome::Error(format!("the {panel} feature isn't ready (see `nova status`)"));
        }
        let output = self.daemon.compositor.as_ref().and_then(|compositor| compositor.focused_output());
        match self.daemon.hub.publish(&Event::Toggle { panel, output }) {
            0 => Outcome::Error("the UI isn't connected to novad, so there's nothing to toggle".into()),
            _ => Outcome::Result(serde_json::Value::Null),
        }
    }

    /// Passes a volume key to the UI, which owns audio (PipeWire through
    /// Quickshell) and the capsule. Volume is core, so it's always ready.
    fn volume(&self, key: VolumeKey) -> Outcome {
        match self.daemon.hub.publish(&Event::Volume { key }) {
            0 => Outcome::Error("the UI isn't connected to novad, so nothing can change the volume".into()),
            _ => Outcome::Result(serde_json::Value::Null),
        }
    }

    /// Work done after the reply is queued.
    fn follow_up(&self, command: Command) {
        match command {
            Command::Subscribe { topics } => {
                for topic in topics {
                    if topic == Topic::Logs {
                        self.daemon.logger.subscribe(self.id);
                        continue;
                    }
                    // Subscribed and sent the current state under the
                    // settings lock, so a reload's event can't come first.
                    let settings = lock(&self.daemon.settings);
                    self.daemon.hub.subscribe(self.id, topic);
                    match topic {
                        Topic::Features => self.send(Event::Features { features: settings.features.clone() }),
                        Topic::Theme => self.send(Event::Theme(settings.theme.clone())),
                        // Without the feature there is nothing to send until
                        // a reload turns it on.
                        Topic::AiUsage => {
                            if let Some(usage) = &settings.ai_usage {
                                self.send(usage.event());
                            }
                        }
                        // Sent once the compositor has said what it shows.
                        Topic::Compositor => {
                            if let Some(compositor) = &self.daemon.compositor {
                                compositor.subscribe(self.id);
                            }
                        }
                        Topic::Lock => self.daemon.session_lock.send_state(&self.daemon.hub, self.id),
                        // A restarted UI picks up a region picker that was open.
                        Topic::Screenshot => self.daemon.screenshots.send_state(&self.daemon.hub, self.id),
                        Topic::Recording => self.daemon.recordings.send_state(&self.daemon.hub, self.id),
                        // Facts about the machine, there whatever the features.
                        Topic::Host => self.send(Event::Host(settings.host())),
                        // Sent while some connection watches them.
                        Topic::Resources => {}
                        Topic::Launcher => {
                            if let Some(launcher) = &settings.launcher {
                                launcher.send_state(self.id);
                            }
                        }
                        Topic::Tray => {
                            if let Some(tray) = &settings.tray {
                                tray.send_state(self.id);
                            }
                        }
                        Topic::Notifications => {
                            if let Some(notifications) = &settings.notifications {
                                notifications.send_state(self.id);
                            }
                        }
                        // Only ever sent as `nova toggle` or `nova volume` asks.
                        Topic::Logs | Topic::Toggle | Topic::Volume => {}
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
            | Command::ShowAiUsage { .. }
            | Command::RefreshAiUsage
            | Command::ReportClaudeUsage { .. }
            | Command::FocusWorkspace { .. }
            | Command::StepWorkspace { .. }
            | Command::Toggle { .. }
            | Command::Lock
            | Command::ReportLocked
            | Command::ReportUnlocked
            | Command::PowerOptions
            | Command::Power { .. }
            | Command::Screenshot { .. }
            | Command::SaveCapture { .. }
            | Command::CancelCapture
            | Command::ScreenshotAction { .. }
            | Command::Record
            | Command::StartRecording { .. }
            | Command::StopRecording
            | Command::PauseRecording { .. }
            | Command::RecordingAction { .. }
            | Command::Volume { .. }
            | Command::WatchResources { .. }
            | Command::EndApp { .. }
            | Command::SearchApps { .. }
            | Command::LaunchApp { .. }
            | Command::PinApp { .. }
            | Command::HideTrayApp { .. }
            | Command::NotificationAction { .. }
            | Command::DismissNotification { .. }
            | Command::ClearNotifications { .. }
            | Command::ReadNotifications
            | Command::SetDoNotDisturb { .. }
            | Command::MuteNotificationApp { .. } => {}
        }
    }

    fn send(&self, message: impl serde::Serialize) {
        match encode(message) {
            Ok(line) => {
                self.queue.push(Outgoing::Line(line));
            }
            Err(e) => self.daemon.logger.error(LOG_SOURCE, &format!("can't encode a message: {e}")),
        }
    }

    /// Waits until everything queued so far is written.
    fn flush(&self) {
        let (done, flushed) = mpsc::sync_channel(1);
        if self.queue.push(Outgoing::Flushed(done)) {
            let _ = flushed.recv();
        }
    }
}

impl Drop for Connection<'_> {
    fn drop(&mut self) {
        self.daemon.hub.unregister(self.id);
        let resources = lock(&self.daemon.settings).resources.clone();
        if let Some(resources) = resources {
            resources.watch(self.id, false);
        }
    }
}

/// Writes a client's queue in order until it closes. A write that fails or
/// stalls past [`WRITE_TIMEOUT`] hangs up on the client: the reader side ends
/// too, so the connection unregisters, and dropping the queue answers any
/// `flush` waiting on it.
fn write_all_queued(mut writer: UnixStream, outgoing: mpsc::Receiver<Outgoing>, logger: &crate::logger::Logger) {
    for message in outgoing {
        match message {
            Outgoing::Line(line) => {
                if let Err(e) = writer.write_all(line.as_bytes()) {
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) {
                        logger.warn(
                            LOG_SOURCE,
                            &format!("a client read nothing for {} s, disconnecting it", WRITE_TIMEOUT.as_secs_f32()),
                        );
                    }
                    let _ = writer.shutdown(std::net::Shutdown::Both);
                    return;
                }
            }
            Outgoing::Flushed(done) => {
                let _ = done.send(());
            }
        }
    }
}

/// The `id` of a request that didn't parse, so the error can be answered.
fn request_id(line: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(line).ok()?.get("id")?.as_u64()
}

/// The pid of the process at the other end of `stream`.
fn peer_pid(stream: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are valid for the size passed.
    let got = unsafe {
        libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&raw mut cred).cast(), &mut len)
    };
    (got == 0).then(|| u32::try_from(cred.pid).ok()).flatten()
}

/// A request's answer: `null`, or its error.
fn outcome(result: Result<(), String>) -> Outcome {
    match result {
        Ok(()) => Outcome::Result(serde_json::Value::Null),
        Err(e) => Outcome::Error(e),
    }
}
