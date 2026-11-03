//! The socket: accepts clients, answers their requests and feeds them the
//! events they subscribed to.

use crate::hub::{ClientId, ClientQueue, Outgoing, Written};
use crate::{Daemon, lock};
use nova_protocol::{
    Capability, CaptureMode, Command, Event, Outcome, Panel, PowerAction, PowerMenu, Response, Topic, VolumeKey, encode,
};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// Any local process can connect: longer request lines close the connection.
pub const MAX_REQUEST_BYTES: u64 = nova_protocol::REQUEST_MAX_BYTES;
/// How long a write may make no progress before the client counts as stalled
/// and is disconnected. Only a client that stops reading with its socket
/// buffer full gets there; the UI reconnects on its own.
pub const WRITE_TIMEOUT: Duration = if cfg!(test) { Duration::from_millis(300) } else { Duration::from_secs(10) };
const SOCKET_DIR_MODE: u32 = 0o700;
/// A request's answer, worked out off the connection's thread.
type SlowWork = Box<dyn FnOnce() -> Outcome + Send>;

const LOG_SOURCE: &str = "server";

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

/// Accepts clients until `stopped` is set; the connection after that (one
/// made to wake it) ends the loop.
pub fn serve(listener: UnixListener, daemon: Arc<Daemon>, stopped: &AtomicBool) {
    for stream in listener.incoming() {
        if stopped.load(Ordering::SeqCst) {
            return;
        }
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
        let written = queue.written();
        std::thread::spawn(move || write_all_queued(writer, outgoing, &written, &logger));
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
        // Answered even without an id: some commands act as they answer.
        match self.answer(&request.command) {
            Answer::Now(outcome) => {
                if let Some(id) = request.id {
                    self.send(Response { id, outcome });
                }
            }
            // Asking logind can take seconds; answering on its own thread
            // keeps this connection reading, so the UI's lock reports and
            // other requests aren't stuck behind it.
            Answer::Later(work) => self.answer_later(request.id, work),
        }
        self.follow_up(request.command);
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
            Command::WatchResources { watching } => {
                done(self.resources().map(|resources| resources.watch(self.id, *watching)))
            }
            Command::EndApp { key } => done(self.resources().and_then(|resources| resources.end_app(key))),
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
            Command::LauncherSearch { query } => {
                value(self.launcher_providers().and_then(|providers| providers.search(query, &self.daemon.logger)))
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
            Command::ShowFolder { folder, select } => done(self.files().and_then(|files| files.show(folder, select))),
            // Answered here, in order with the connection's other requests,
            // so a close or a hang-up never overtakes the open it follows.
            Command::FilesOpen { view, path, sort, hidden } => {
                done(self.files().and_then(|files| files.open(self.id, *view, path, *sort, *hidden)))
            }
            Command::FilesClose { view } => done(self.files().map(|files| files.close(self.id, *view))),
            Command::FilesTransfer { job, mode, sources, folder } => {
                done(self.files().and_then(|files| files.transfer(self.id, *job, *mode, sources, folder)))
            }
            Command::FilesTransferCancel { job } => {
                done(self.files().map(|files| files.cancel_transfer(self.id, *job)))
            }
            Command::FilesTransferAnswer { job, clash, answer, all } => {
                done(self.files().and_then(|files| files.answer_clash(self.id, *job, *clash, *answer, *all)))
            }
            Command::AttachFiles { attached } => done(self.files().map(|files| files.attach(self.id, *attached))),
            Command::FilesLaunch { path, app } => {
                let (path, app) = (path.clone(), app.clone());
                return self.opener_later(move |opener| match app {
                    Some(app) => opener.open_with(&path, &app),
                    None => opener.open(&path),
                });
            }
            // Answered off the connection's thread: the files found are
            // looked at, and a slow drive must not hold up its other requests.
            Command::FilesSearch { query } => {
                let query = query.clone();
                return match self.files() {
                    Ok(files) => {
                        let searcher = files.searcher();
                        Answer::later(move || value(searcher.search(&query)))
                    }
                    Err(e) => Answer::Now(Outcome::Error(e)),
                };
            }
            Command::FilesOpenWith { path } => {
                let path = path.clone();
                return self.opener_later(move |opener| opener.choices(&path));
            }
            Command::FilesSetDefault { path, app } => {
                let (path, app) = (path.clone(), app.clone());
                return self.opener_later(move |opener| opener.set_default(&path, &app));
            }
            Command::FilesMount { drive } => {
                let drive = drive.clone();
                return self.files_later(move |files| files.mount(&drive));
            }
            Command::FilesEject { drive } => {
                let drive = drive.clone();
                return self.files_later(move |files| files.eject(&drive));
            }
            Command::FilesTree { folder, opened, closed, hidden } => {
                let (folder, opened, closed, hidden) = (folder.clone(), opened.clone(), closed.clone(), *hidden);
                return self.files_later(move |files| {
                    files.tree(&crate::files::tree::Request {
                        folder: &folder,
                        opened: &opened,
                        closed: &closed,
                        hidden,
                    })
                });
            }
            Command::FilesPlaces => return self.files_later(|files| Ok(files.places())),
            Command::FilesSameDrive { sources, folder } => {
                let (sources, folder) = (sources.clone(), folder.clone());
                return self.files_later(move |_| crate::files::same_drive(&sources, &folder));
            }
            Command::FilesTrash { paths } => {
                let paths = paths.clone();
                return self.files_later(move |files| files.trash().trash(&paths));
            }
            Command::FilesDelete { paths } => {
                let paths = paths.clone();
                return self.files_later(move |_| crate::files::Trash::delete(&paths));
            }
            Command::FilesTrashList => return self.files_later(|files| Ok(files.trash().list())),
            Command::FilesRestore { ids } => {
                let ids = ids.clone();
                return self.files_later(move |files| files.trash().restore(&ids));
            }
            Command::FilesTrashDelete { ids } => {
                let ids = ids.clone();
                return self.files_later(move |files| files.trash().erase(&ids));
            }
            Command::FilesEmptyTrash => return self.files_later(|files| files.trash().empty()),
            Command::FilesProperties { path } => {
                let path = path.clone();
                return self.files_later(move |files| files.properties(&path));
            }
            Command::FilesSetAccess { path, access } => {
                let (path, access) = (path.clone(), *access);
                return self.files_later(move |files| files.set_access(&path, access));
            }
            // Answered here, in order with a cancel that follows it.
            Command::FilesMeasure { job, paths } => {
                done(self.files().and_then(|files| files.measure(self.id, *job, paths)))
            }
            Command::FilesMeasureCancel { job } => done(self.files().map(|files| files.cancel_measure(self.id, *job))),
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

    fn resources(&self) -> Result<Arc<crate::resources::Resources>, String> {
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

    fn files(&self) -> Result<Arc<crate::files::Files>, String> {
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
    /// app it focuses or starts (see `launcher::providers`).
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
            let how = crate::launcher::providers::Run {
                output: output.as_deref(),
                new_window,
                compositor: compositor.as_deref(),
                tray: tray.as_deref(),
                logger: &logger,
            };
            done(provider.run(&id, &how))
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

    /// Runs `work` with Nova Files' opener on its own thread: it reads the
    /// file and may wait for an app to start.
    fn opener_later<T: serde::Serialize>(
        &self,
        work: impl FnOnce(&crate::apps::opening::Opener) -> Result<T, String> + Send + 'static,
    ) -> Answer {
        match self.files() {
            Ok(files) => {
                let opener = files.opener();
                Answer::later(move || value(work(&opener)))
            }
            Err(e) => Answer::Now(Outcome::Error(e)),
        }
    }

    /// Runs `work` with Nova Files on its own thread: it reads folders and
    /// moves or deletes files.
    fn files_later<T: serde::Serialize>(
        &self,
        work: impl FnOnce(&crate::files::Files) -> Result<T, String> + Send + 'static,
    ) -> Answer {
        match self.files() {
            Ok(files) => Answer::later(move || value(work(&files))),
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
                        // Sent once the compositor has said what it shows.
                        Topic::Compositor => {
                            if let Some(compositor) = &self.daemon.compositor {
                                compositor.subscribe(self.id);
                            }
                        }
                        // Facts about the machine, there whatever the features.
                        Topic::Host => self.send(Event::Host(settings.host())),
                        // Sent while some connection watches them.
                        Topic::Resources => {}
                        // A module's: without its feature there is nothing to
                        // send until a reload turns it on.
                        Topic::AiUsage
                        | Topic::Lock
                        | Topic::Screenshot
                        | Topic::Recording
                        | Topic::Launcher
                        | Topic::Tray
                        | Topic::Notifications
                        | Topic::Media
                        | Topic::Peripherals
                        | Topic::Network
                        | Topic::Bluetooth
                        | Topic::Files => settings.modules.send_state(topic, self.id),
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
            | Command::WatchResources { .. }
            | Command::EndApp { .. }
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
            | Command::BluetoothAnswer { .. }
            | Command::ShowFolder { .. }
            | Command::FilesOpen { .. }
            | Command::FilesClose { .. }
            | Command::FilesTransfer { .. }
            | Command::FilesSameDrive { .. }
            | Command::FilesTransferCancel { .. }
            | Command::FilesTransferAnswer { .. }
            | Command::AttachFiles { .. }
            | Command::FilesLaunch { .. }
            | Command::FilesOpenWith { .. }
            | Command::FilesSetDefault { .. }
            | Command::FilesSearch { .. }
            | Command::FilesTrash { .. }
            | Command::FilesDelete { .. }
            | Command::FilesTrashList
            | Command::FilesRestore { .. }
            | Command::FilesTrashDelete { .. }
            | Command::FilesEmptyTrash
            | Command::FilesMount { .. }
            | Command::FilesEject { .. }
            | Command::FilesProperties { .. }
            | Command::FilesMeasure { .. }
            | Command::FilesMeasureCancel { .. }
            | Command::FilesSetAccess { .. } => {}
            Command::FilesTree { .. } | Command::FilesPlaces => {}
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
        lock(&self.daemon.settings).modules.client_gone(self.id);
    }
}

/// Writes a client's queue in order until it closes. A write that fails or
/// stalls past [`WRITE_TIMEOUT`] hangs up on the client: the reader side ends
/// too, so the connection unregisters, and dropping the queue answers any
/// `flush` waiting on it.
fn write_all_queued(
    mut writer: UnixStream,
    outgoing: mpsc::Receiver<Outgoing>,
    written: &Written,
    logger: &crate::logger::Logger,
) {
    for message in outgoing {
        match message {
            Outgoing::Line(line) => {
                written.line(line.len());
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

/// A request's answer, now or from work on another thread.
enum Answer {
    Now(Outcome),
    Later(SlowWork),
}

impl Answer {
    fn later(work: impl FnOnce() -> Outcome + Send + 'static) -> Self {
        Self::Later(Box::new(work))
    }
}

/// An answer that's `null`, or its error.
fn done(result: Result<(), String>) -> Outcome {
    value(result)
}

/// An answer with `result`'s value, or its error.
fn value(result: Result<impl serde::Serialize, String>) -> Outcome {
    match result.and_then(|value| serde_json::to_value(value).map_err(|e| e.to_string())) {
        Ok(value) => Outcome::Result(value),
        Err(e) => Outcome::Error(e),
    }
}
