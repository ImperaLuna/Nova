//! What novad answers on the socket: one route per message type in
//! [`router`]. `nova_protocol::Command` is the shared list the feature moves
//! take apart (IMP-368); a feature that owns its commands (its api crate's
//! `Command`) has its own route, which calls into its module. The socket
//! itself is `nova-socket`'s.

use crate::hub::ClientId;
use crate::{Daemon, lock};
use nova_protocol::{
    Capability, CaptureMode, Command, Event, Outcome, Panel, PowerAction, PowerMenu, VolumeKey, topics,
};
use nova_socket::{Answer, Caller, Router, done, value};
use std::sync::{Arc, mpsc};

const LOG_SOURCE: &str = "server";

/// The routes of every message novad serves. A moved feature adds its
/// route in its place in the list below (by its feature name), and its
/// commands leave `nova_protocol::Command`.
pub fn router() -> Router<Daemon> {
    Router::new()
        .route::<Shared>(
            |daemon, caller, Shared(command)| Connection::new(daemon, caller).answer(command),
            |daemon, caller, Shared(command)| Connection::new(daemon, caller).follow_up(command),
        )
        .route::<nova_resources_api::Command>(crate::resources::answer, |_, _, _| {})
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
    // The connecting process, from the kernel (SO_PEERCRED).
    peer_pid: Option<u32>,
}

impl<'a> Connection<'a> {
    fn new(daemon: &'a Daemon, caller: &'a Caller) -> Self {
        Self { daemon, caller, id: caller.id(), peer_pid: caller.peer_pid() }
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
            Command::RefreshAiUsage => done(self.ai_usage().map(|usage| usage.refresh())),
            Command::ShowAiUsage { provider } => done(self.ai_usage().and_then(|usage| usage.show(*provider))),
            Command::ReportClaudeUsage { limits } => {
                done(self.ai_usage().and_then(|usage| usage.report_claude(limits.clone())))
            }
            Command::FocusWorkspace { id } => {
                done(self.compositor().and_then(|compositor| compositor.focus_workspace(*id)))
            }
            Command::StepWorkspace { output, step } => {
                done(self.compositor().and_then(|compositor| compositor.step_workspace(output, *step)))
            }
            Command::Toggle { panel } => self.toggle(*panel),
            Command::Volume { key } => self.volume(*key),
            Command::Lock => done(
                self.require_feature(crate::session_lock::FEATURE_ID)
                    .and_then(|()| self.session_lock().lock(&self.daemon.hub)),
            ),
            Command::ReportLocked => done(self.sent_by_the_ui().map(|()| self.session_lock().secured())),
            // Even with the feature off: the screen was locked while it was on.
            Command::ReportUnlocked => {
                done(self.sent_by_the_ui().map(|()| self.session_lock().unlocked(&self.daemon.hub)))
            }
            Command::PowerOptions => return self.power_options(),
            Command::Power { action } => return self.power(*action),
            Command::Screenshot { mode } => return self.screenshot(*mode),
            Command::SaveCapture { output, rect } => return self.save_capture(output.clone(), *rect),
            // Even with the feature off, so a reload can't strand the picker.
            Command::CancelCapture => {
                self.screenshots().cancel();
                done(Ok(()))
            }
            Command::ScreenshotAction { id, action } => done(self.screenshots().act(*id, *action)),
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
            // A provider's own mode (files) waits on another app (Nova
            // Files), so it's answered off the connection's thread: a slow
            // answer must not hold up other requests. The mixed providers answer from memory.
            Command::LauncherSearch { query, mode } => {
                let providers = match self.launcher_providers() {
                    Ok(providers) => providers,
                    Err(e) => return Answer::Now(Outcome::Error(e)),
                };
                if mode.is_none() {
                    return Answer::Now(value(providers.search(query, None, &self.daemon.logger)));
                }
                let (query, mode, logger) = (query.clone(), mode.clone(), Arc::clone(&self.daemon.logger));
                return Answer::later(move || value(providers.search(&query, mode.as_deref(), &logger)));
            }
            Command::AppIcon { icon_name, program, name } => {
                let apps = Arc::clone(&lock(&self.daemon.settings).context.apps);
                value(apps.app_icon(&self.daemon.sources.icons, icon_name, program, name))
            }
            Command::LauncherRun { provider, id, output, new_window } => {
                return self.launcher_run(provider, id, output.clone(), *new_window);
            }
            Command::PinApp { id, pinned } => done(self.launcher().and_then(|launcher| launcher.pin(id, *pinned))),
            Command::HideTrayApp { id, hidden } => done(self.tray().and_then(|tray| tray.hide(id, *hidden))),
            Command::ActivateTrayItem { address } => return self.activate_tray_item(address),
            Command::TrayMenuPicked { address } => return self.tray_menu_picked(address),
            Command::OpenNotification { id, output } => return self.open_notification(*id, output.clone()),
            Command::NotificationAction { id, action } => {
                done(self.notifications().and_then(|notifications| notifications.action(*id, Some(action))))
            }
            Command::DismissNotification { id } => {
                done(self.notifications().map(|notifications| notifications.dismiss(*id)))
            }
            Command::ClearNotifications { app } => {
                done(self.notifications().map(|notifications| notifications.clear(app.as_deref())))
            }
            Command::ReadNotifications => done(self.notifications().map(|notifications| notifications.read(None))),
            Command::SetDoNotDisturb { on } => {
                done(self.notifications().map(|notifications| notifications.set_do_not_disturb(*on)))
            }
            Command::MuteNotificationApp { app, muted } => {
                done(self.notifications().and_then(|notifications| notifications.mute(app, *muted)))
            }
            // The service knows which wallpapers need which feature.
            Command::SetWallpaper { path, output } => done(self.wallpaper().set(path, output.as_deref())),
            Command::Wallpapers => value(self.wallpaper().current()),
            Command::WallpaperFolder => return self.wallpaper_folder(),
            Command::MediaShow { player } => done(self.media().and_then(|media| media.show(player))),
            Command::MediaControl { player, action } => {
                let (player, action) = (player.clone(), *action);
                return self.media_later(move |media| done(media.control(&player, action)));
            }
            Command::MediaSeek { player, position_us } => {
                let (player, position_us) = (player.clone(), *position_us);
                return self.media_later(move |media| done(media.seek(&player, position_us)));
            }
            Command::MediaFocus { player } => return self.media_focus(player),
            Command::MediaWindow { player } => return self.media_window(player),
            Command::SetWifi { enabled } => {
                let enabled = *enabled;
                return self.network_later(move |network| done(network.set_wifi(enabled)));
            }
            Command::ScanWifi => return self.network_later(|network| done(network.scan())),
            Command::ConnectWifi { ssid, password } => {
                let (ssid, password) = (ssid.clone(), password.clone());
                return self.network_later(move |network| done(network.connect(&ssid, password.as_ref())));
            }
            Command::DisconnectWifi => return self.network_later(|network| done(network.disconnect())),
            Command::ForgetWifi { ssid } => {
                let ssid = ssid.clone();
                return self.network_later(move |network| done(network.forget(&ssid)));
            }
            Command::PreferNetwork { kind } => done(self.network().and_then(|network| network.prefer(*kind))),
            Command::WatchNetworkTraffic { watching } => {
                done(self.network().map(|network| network.watch_traffic(self.id, *watching)))
            }
            Command::WatchBluetooth { watching } => {
                done(self.bluetooth().map(|bluetooth| bluetooth.watch(self.id, *watching)))
            }
            Command::BluetoothPower { on } => {
                let on = *on;
                return self.bluetooth_later(move |bluetooth| bluetooth.power(on));
            }
            Command::BluetoothConnect { address } => {
                let address = address.clone();
                return self.bluetooth_later(move |bluetooth| bluetooth.connect(&address));
            }
            Command::BluetoothDisconnect { address } => {
                let address = address.clone();
                return self.bluetooth_later(move |bluetooth| bluetooth.disconnect(&address));
            }
            Command::BluetoothForget { address } => {
                let address = address.clone();
                return self.bluetooth_later(move |bluetooth| bluetooth.forget(&address));
            }
            Command::BluetoothAnswer { id, answer } => {
                done(self.bluetooth().and_then(|bluetooth| bluetooth.answer(*id, answer.clone())))
            }
        };
        Answer::Now(outcome)
    }

    // The modules, or why there's none (`Daemon::module`).

    fn ai_usage(&self) -> Result<Arc<crate::ai_usage::Usage>, String> {
        self.daemon.module()
    }

    fn launcher(&self) -> Result<Arc<crate::launcher::Launcher>, String> {
        self.daemon.module()
    }

    fn tray(&self) -> Result<Arc<crate::tray::Tray>, String> {
        self.daemon.module()
    }

    fn notifications(&self) -> Result<Arc<crate::notifications::Notifications>, String> {
        self.daemon.module()
    }

    fn media(&self) -> Result<Arc<crate::media::Media>, String> {
        self.daemon.module()
    }

    fn network(&self) -> Result<Arc<crate::network::Network>, String> {
        self.daemon.module()
    }

    // Kept for novad's life, whatever the settings.

    fn session_lock(&self) -> Arc<crate::session_lock::SessionLock> {
        self.daemon.session_lock()
    }

    fn screenshots(&self) -> Arc<crate::screenshot::Screenshots> {
        Arc::clone(&self.daemon.kept.screenshots)
    }

    fn recordings(&self) -> Arc<crate::recording::Recordings> {
        Arc::clone(&self.daemon.kept.recordings)
    }

    fn wallpaper(&self) -> Arc<crate::wallpaper::Wallpaper> {
        Arc::clone(&self.daemon.kept.wallpaper)
    }

    /// Runs `work` on its own thread: NetworkManager may take a while.
    fn network_later(&self, work: impl FnOnce(&crate::network::Network) -> Outcome + Send + 'static) -> Answer {
        match self.network() {
            Ok(network) => Answer::later(move || work(&network)),
            Err(e) => Answer::Now(Outcome::Error(e)),
        }
    }

    fn bluetooth(&self) -> Result<Arc<crate::bluetooth::Bluetooth>, String> {
        self.daemon.module()
    }

    fn compositor(&self) -> Result<&crate::compositor::Compositor, String> {
        self.daemon.compositor.as_deref().ok_or_else(|| "novad doesn't know this compositor".into())
    }

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
            false => Err(crate::features::not_ready(id)),
        }
    }

    fn power_options(&self) -> Answer {
        if let Err(e) = self.require_feature(crate::power::FEATURE_ID) {
            return Answer::Now(Outcome::Error(e));
        }
        let lock = self.require_feature(crate::session_lock::FEATURE_ID);
        let log_out = match &self.daemon.compositor {
            Some(compositor) if compositor.can(Capability::LogOut) => Ok(()),
            _ => Err("Not on this compositor".into()),
        };
        let logger = Arc::clone(&self.daemon.logger);
        Answer::later(move || {
            let menu = PowerMenu {
                options: crate::power::options(lock, log_out, &logger),
                booted_at: crate::procfs::boot_time(),
            };
            value(Ok(menu))
        })
    }

    /// Runs a power menu action. The menu asks over the UI's own
    /// connection, which can't report the lock screen covering the screens
    /// while this waits, so locking here doesn't wait for it.
    fn power(&self, action: PowerAction) -> Answer {
        if let Err(e) = self.require_feature(crate::power::FEATURE_ID) {
            return Answer::Now(Outcome::Error(e));
        }
        self.daemon.logger.info(LOG_SOURCE, &format!("power menu: {action:?}"));
        match action {
            PowerAction::Lock => Answer::Now(done(
                self.require_feature(crate::session_lock::FEATURE_ID)
                    .and_then(|()| self.session_lock().request(&self.daemon.hub)),
            )),
            PowerAction::LogOut => {
                Answer::Now(done(self.compositor().and_then(crate::compositor::Compositor::log_out)))
            }
            PowerAction::Suspend | PowerAction::Reboot | PowerAction::PowerOff => self.power_logind(action),
        }
    }

    /// Suspend, restart or shut down through logind; answers with logind's
    /// own result. A suspend locks the screen first through novad's sleep
    /// inhibitor ([`crate::logind`]), as one from anywhere else does.
    fn power_logind(&self, action: PowerAction) -> Answer {
        let logger = Arc::clone(&self.daemon.logger);
        Answer::later(move || {
            done(
                crate::power::run_logind(action)
                    .inspect_err(|e| logger.warn(LOG_SOURCE, &format!("{action:?} failed: {e}"))),
            )
        })
    }

    /// The ready features' launcher providers, while the launcher runs.
    fn launcher_providers(&self) -> Result<crate::launcher::providers::Host, String> {
        self.launcher()?;
        let settings = lock(&self.daemon.settings);
        Ok(crate::launcher::providers::Host::new(&settings.features, |view| {
            crate::launcher::providers::find(view, &settings.modules)
        }))
    }

    /// Runs what the launcher's provider `provider` found as `id`, like an
    /// app it focuses or starts (see `launcher::providers`). Answers once it
    /// is underway: the launcher waits for the answer to close, and an app
    /// in the tray may take seconds to show its window. A failure after
    /// that is only logged.
    fn launcher_run(&self, provider: &str, id: &str, output: Option<String>, new_window: bool) -> Answer {
        let provider = match self.launcher_providers().and_then(|providers| providers.provider(provider)) {
            Ok(provider) => provider,
            Err(e) => return Answer::Now(Outcome::Error(e)),
        };
        let compositor = self.daemon.compositor.clone();
        let tray = self.tray().ok();
        let logger = Arc::clone(&self.daemon.logger);
        let id = id.to_owned();
        Answer::later(move || {
            let (answer, answered) = mpsc::channel();
            std::thread::spawn(move || {
                let early = answer.clone();
                let underway = move || {
                    let _ = early.send(Ok(()));
                };
                let how = crate::launcher::providers::Run {
                    output: output.as_deref(),
                    new_window,
                    compositor: compositor.as_deref(),
                    tray: tray.as_deref(),
                    logger: &logger,
                    underway: &underway,
                };
                // Unheard once it was underway; starting the app logs its
                // own failures.
                let _ = answer.send(provider.run(&id, &how));
            });
            done(answered.recv().unwrap_or_else(|_| Err("the launch stopped without an answer".into())))
        })
    }

    /// A click on the notification `id`, on `output`. Its app hears of it
    /// and shows its window, which takes focus once it shows: niri gives
    /// it none, as the app gets no activation token from the click. Nothing
    /// starts when none shows: the app took the click. An app that can't
    /// hear of it (gone, or does nothing on a click) is opened instead, as
    /// the launcher does.
    fn open_notification(&self, id: u32, output: Option<String>) -> Answer {
        let clicked = match self.notifications().and_then(|notifications| notifications.click(id)) {
            Ok(clicked) => clicked,
            Err(e) => return Answer::Now(Outcome::Error(e)),
        };
        let (Some(app), Ok(launcher)) = (clicked.app_id, self.launcher()) else { return Answer::Now(done(Ok(()))) };
        if !clicked.heard {
            return self.launcher_run(nova_protocol::PROVIDER_APPS, &app, output, false);
        }
        let (Some(ids), Some(compositor)) = (launcher.window_ids(&app), self.daemon.compositor.clone()) else {
            return Answer::Now(done(Ok(())));
        };
        let logger = Arc::clone(&self.daemon.logger);
        Answer::later(move || {
            crate::launcher::focus_shown(
                &app,
                &ids,
                &compositor,
                output.as_deref(),
                crate::launcher::SHOWN_WINDOW_WAIT,
                &logger,
            );
            done(Ok(()))
        })
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
    fn activate_tray_item(&self, address: &str) -> Answer {
        let tray = match self.tray() {
            Ok(tray) => tray,
            Err(e) => return Answer::Now(Outcome::Error(e)),
        };
        let launcher = self.launcher().ok();
        let compositor = self.daemon.compositor.clone();
        let logger = Arc::clone(&self.daemon.logger);
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
    fn tray_menu_picked(&self, address: &str) -> Answer {
        let found = self.tray().and_then(|tray| tray.app_at(address)).map(|app| {
            let launcher = self.launcher().ok()?;
            let app = app?;
            Some((launcher.window_ids(&app)?, app))
        });
        let (ids, app) = match found {
            Ok(Some(found)) => found,
            Ok(None) => return Answer::Now(Outcome::Result(serde_json::Value::Null)),
            Err(e) => return Answer::Now(Outcome::Error(e)),
        };
        let Some(compositor) = self.daemon.compositor.clone() else {
            return Answer::Now(Outcome::Result(serde_json::Value::Null));
        };
        if compositor.app_workspace(&ids).is_ok_and(|workspace| workspace.is_none()) {
            let logger = Arc::clone(&self.daemon.logger);
            std::thread::spawn(move || {
                crate::launcher::focus_shown(&app, &ids, &compositor, None, crate::launcher::SHOWN_WINDOW_WAIT, &logger)
            });
        }
        Answer::Now(Outcome::Result(serde_json::Value::Null))
    }

    /// Runs `work` with the media service on its own thread: it waits on a
    /// player, or the compositor, to answer.
    /// Lists the wallpaper folder on its own thread: reading each file's
    /// header takes a moment in a big folder, or one on a slow disk.
    fn wallpaper_folder(&self) -> Answer {
        let folder = self.require_feature(crate::wallpaper::FEATURE_ID).and_then(|()| {
            let settings = lock(&self.daemon.settings);
            settings.wallpaper_config().folder()
        });
        match folder {
            Ok(folder) => Answer::later(move || value(crate::wallpaper_folder::list(&folder))),
            Err(e) => Answer::Now(Outcome::Error(e)),
        }
    }

    /// Runs `work` with Bluetooth on its own thread: BlueZ, the device and
    /// the user's answer to pairing take their time.
    fn bluetooth_later(
        &self,
        work: impl FnOnce(&crate::bluetooth::Bluetooth) -> Result<(), String> + Send + 'static,
    ) -> Answer {
        match self.bluetooth() {
            Ok(bluetooth) => Answer::later(move || done(work(&bluetooth))),
            Err(e) => Answer::Now(Outcome::Error(e)),
        }
    }

    fn media_later(&self, work: impl FnOnce(&crate::media::Media) -> Outcome + Send + 'static) -> Answer {
        match self.media() {
            Ok(media) => Answer::later(move || work(&media)),
            Err(e) => Answer::Now(Outcome::Error(e)),
        }
    }

    /// Brings `player`'s window forward: where it is, as the launcher does
    /// (IMP-114), else by asking the player to raise itself. Answers whether
    /// either worked.
    fn media_focus(&self, player: &str) -> Answer {
        let compositor = self.daemon.compositor.clone();
        let logger = Arc::clone(&self.daemon.logger);
        let player = player.to_owned();
        self.media_later(move |media| {
            value(media.window_ids(&player).and_then(|ids| {
                let focused = match compositor.as_deref() {
                    Some(compositor) if !ids.is_empty() => compositor.focus_app(&ids, None).unwrap_or_else(|e| {
                        logger.warn(LOG_SOURCE, &format!("can't focus {player}'s window: {e}"));
                        false
                    }),
                    _ => false,
                };
                if focused { Ok(true) } else { media.raise(&player) }
            }))
        })
    }

    /// The name of the workspace `player`'s window is on, or `null`.
    fn media_window(&self, player: &str) -> Answer {
        let compositor = self.daemon.compositor.clone();
        let player = player.to_owned();
        self.media_later(move |media| {
            value(media.window_ids(&player).and_then(|ids| match compositor.as_deref() {
                Some(compositor) if !ids.is_empty() => compositor.app_workspace(&ids),
                _ => Ok(None),
            }))
        })
    }

    /// Opens the region picker on every screen (or closes it), or saves the
    /// focused screen.
    fn screenshot(&self, mode: CaptureMode) -> Answer {
        if let Err(e) = self.require_feature(crate::screenshot::FEATURE_ID) {
            return Answer::Now(Outcome::Error(e));
        }
        let screenshots = self.screenshots();
        let compositor = self.daemon.compositor.clone();
        let folder = lock(&self.daemon.settings).screenshot_folder();
        Answer::later(move || {
            done(match mode {
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
            })
        })
    }

    /// Saves the region the user picked.
    fn save_capture(&self, output: String, rect: nova_protocol::PixelRect) -> Answer {
        if let Err(e) = self.require_feature(crate::screenshot::FEATURE_ID) {
            return Answer::Now(Outcome::Error(e));
        }
        let screenshots = self.screenshots();
        let folder = lock(&self.daemon.settings).screenshot_folder();
        Answer::later(move || done(folder.and_then(|folder| screenshots.save_region(&output, rect, &folder))))
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
            | Command::SetWallpaper { .. }
            | Command::Wallpapers
            | Command::WallpaperFolder
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
            | Command::OpenLog
            | Command::Volume { .. }
            | Command::LauncherSearch { .. }
            | Command::AppIcon { .. }
            | Command::LauncherRun { .. }
            | Command::PinApp { .. }
            | Command::HideTrayApp { .. }
            | Command::ActivateTrayItem { .. }
            | Command::TrayMenuPicked { .. }
            | Command::NotificationAction { .. }
            | Command::DismissNotification { .. }
            | Command::ClearNotifications { .. }
            | Command::ReadNotifications
            | Command::OpenNotification { .. }
            | Command::SetDoNotDisturb { .. }
            | Command::MuteNotificationApp { .. }
            | Command::MediaControl { .. }
            | Command::MediaSeek { .. }
            | Command::MediaShow { .. }
            | Command::MediaFocus { .. }
            | Command::MediaWindow { .. }
            | Command::SetWifi { .. }
            | Command::ScanWifi
            | Command::ConnectWifi { .. }
            | Command::DisconnectWifi
            | Command::ForgetWifi { .. }
            | Command::PreferNetwork { .. }
            | Command::WatchNetworkTraffic { .. }
            | Command::BluetoothPower { .. }
            | Command::WatchBluetooth { .. }
            | Command::BluetoothConnect { .. }
            | Command::BluetoothDisconnect { .. }
            | Command::BluetoothForget { .. }
            | Command::BluetoothAnswer { .. } => {}
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
    use std::path::Path;

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
        let protocol_js = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/daemon/protocol.js");
        let js = std::fs::read_to_string(protocol_js).unwrap();
        let mut named: Vec<&str> =
            js.lines().filter_map(|line| line.strip_prefix("var CMD_")?.split('"').nth(1)).collect();
        let mut routed = super::router().names().to_vec();
        assert!(!named.is_empty(), "protocol.js names no commands");
        named.sort_unstable();
        routed.sort_unstable();
        assert_eq!(routed, named);
    }
}
