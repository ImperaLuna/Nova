use crate::pam;
use crate::prompt::{Key, Secret, Verdict};
use crate::surface::LockSurface;
use crate::tokens::DURATION_UNLOCK;
use futures::FutureExt;
use gpui::{
    App, AppContext, Context, DisplayId, Entity, Global, Keystroke, SessionLockState, SharedString, Subscription, Task,
    WindowHandle,
};
use nova_desktop_info::{Desktop, Event as DesktopEvent, HYPRLAND_LOCK_RESTORE};
use nova_lock_api::{Command, Event, TOPIC};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Daemon, Live, Refusal};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const USER_VARS: [&str; 2] = ["USER", "LOGNAME"];
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Held(Option<Entity<LockScreen>>);

impl Global for Held {}

#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    pub user: SharedString,
    pub typed: usize,
    pub checking: bool,
    pub failed: bool,
    pub ring_changed: Option<Instant>,
    pub message: Option<&'static str>,
    pub shaken: Option<Instant>,
    pub unlocking: Option<Instant>,
}

pub struct LockScreen {
    held: bool,
    confirmed: bool,
    unreported: bool,
    user: SharedString,
    typed: Secret,
    check: Option<Task<()>>,
    pam_busy: Arc<AtomicBool>,
    failed: bool,
    ring_changed: Option<Instant>,
    message: Option<&'static str>,
    shaken: Option<Instant>,
    unlocking: Option<Instant>,
    release: Option<Task<()>>,
    windows: Vec<(DisplayId, WindowHandle<LockSurface>)>,
    opening: Vec<DisplayId>,
    unopened: Vec<DisplayId>,
    desktop: Entity<Live<Desktop>>,
    _followed: Vec<Subscription>,
}

impl LockScreen {
    pub fn get(cx: &mut App) -> Entity<Self> {
        match cx.default_global::<Held>().0.clone() {
            Some(held) => held,
            None => Self::start(cx),
        }
    }

    fn start(cx: &mut App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let desktop = Live::follow(nova_desktop_info::TOPIC, cx, |DesktopEvent::Compositor(desktop)| Some(desktop));
            let this = cx.weak_entity();
            let asked = Daemon::on_event(TOPIC, cx, move |Event::Lock { locked }, cx| {
                if locked {
                    let _ = this.update(cx, |lock, cx| lock.asked(cx));
                }
            });
            let this = cx.weak_entity();
            let answered = cx.on_session_lock_change(move |cx| {
                let _ = this.update(cx, |lock, cx| lock.compositor_answered(cx));
            });
            let mut followed = vec![
                asked,
                answered,
                cx.observe(&desktop, |lock: &mut Self, _, cx| {
                    if lock.held {
                        cx.notify();
                    }
                }),
                cx.on_release(|lock: &mut Self, cx| lock.close_windows(cx)),
            ];
            if let Some(screens) = Stage::screens(cx) {
                followed.push(cx.observe(&screens, |lock: &mut Self, _, cx| {
                    lock.unopened.clear();
                    lock.sync_windows(cx);
                    cx.notify();
                }));
            }
            Self {
                held: false,
                confirmed: false,
                unreported: false,
                user: user_name().into(),
                typed: Secret::default(),
                check: None,
                pam_busy: Arc::default(),
                failed: false,
                ring_changed: None,
                message: None,
                shaken: None,
                unlocking: None,
                release: None,
                windows: Vec::new(),
                opening: Vec::new(),
                unopened: Vec::new(),
                desktop,
                _followed: followed,
            }
        })
    }

    fn asked(&mut self, cx: &mut Context<Self>) {
        if self.unreported && !self.held {
            self.report_unlocked(cx);
            return;
        }
        self.lock(cx);
    }

    fn lock(&mut self, cx: &mut Context<Self>) {
        if self.held {
            if self.unlocking.is_some() {
                self.reset();
                cx.notify();
            }
            if cx.session_lock_state() == SessionLockState::Locked {
                Daemon::send(Command::ReportLocked, cx);
            }
            return;
        }
        if self.user.is_empty() {
            eprintln!("ERROR nova-ui: not locking: neither {} names the user", USER_VARS.join(" nor "));
            return;
        }
        if let Err(error) = cx.lock_session() {
            eprintln!("ERROR nova-ui: can't lock the session: {error:#}");
            return;
        }
        self.held = true;
        self.reset();
        let this = cx.entity();
        cx.set_global(Held(Some(this)));
        self.sync_windows(cx);
        cx.notify();
    }

    fn compositor_answered(&mut self, cx: &mut Context<Self>) {
        if !self.held {
            return;
        }
        match cx.session_lock_state() {
            SessionLockState::Locked => {
                eprintln!("INFO nova-ui: session locked");
                self.confirmed = true;
                Daemon::send(Command::ReportLocked, cx);
            }
            SessionLockState::Unlocked if self.confirmed => {
                eprintln!("WARN nova-ui: the compositor ended the session lock");
                self.report_unlocked(cx);
                self.let_go(cx);
            }
            SessionLockState::Unlocked => {
                eprintln!(
                    "ERROR nova-ui: the compositor refused the session lock. On Hyprland a lock screen that \
                     restarted can lock again only with {HYPRLAND_LOCK_RESTORE} = true, which Nova's Hyprland \
                     rules set; `nova doctor` checks it"
                );
                self.let_go(cx);
            }
            SessionLockState::Pending => {}
        }
    }

    fn let_go(&mut self, cx: &mut Context<Self>) {
        (self.held, self.confirmed) = (false, false);
        self.reset();
        self.close_windows(cx);
        cx.set_global(Held(None));
        cx.notify();
    }

    fn report_unlocked(&mut self, cx: &mut Context<Self>) {
        self.unreported = false;
        let reported = Daemon::call::<()>(Command::ReportUnlocked, cx);
        cx.spawn(async move |this, cx| {
            if let Err(refusal) = reported.await {
                eprintln!("WARN nova-ui: report_unlocked failed: {refusal}");
                let link_down = matches!(refusal, Refusal::NotConnected | Refusal::WentAway);
                let _ = this.update(cx, |lock, _| lock.unreported = link_down);
            }
        })
        .detach();
    }

    fn reset(&mut self) {
        self.unopened.clear();
        self.typed.clear();
        (self.check, self.release) = (None, None);
        (self.failed, self.message) = (false, None);
        (self.ring_changed, self.shaken, self.unlocking) = (None, None, None);
    }

    pub fn key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        if self.check.is_some() || self.unlocking.is_some() {
            return;
        }
        let held = keystroke.modifiers;
        let shortcut = held.control || held.alt || held.platform;
        match Key::of(&keystroke.key, keystroke.key_char.as_deref(), shortcut) {
            Key::Submit => self.submit(cx),
            Key::Clear => self.typed.clear(),
            Key::Erase => self.typed.pop(),
            Key::Type(text) => {
                self.typed.push(text);
                self.set_failed(false);
                self.message = None;
            }
            Key::Ignored => return,
        }
        cx.notify();
    }

    fn set_failed(&mut self, failed: bool) {
        if failed != self.failed {
            self.failed = failed;
            self.ring_changed = Some(Instant::now());
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.typed.is_empty() {
            return;
        }
        self.set_failed(false);
        self.message = None;
        let (user, password) = (self.user.clone(), self.typed.take());
        if self.pam_busy.swap(true, Ordering::SeqCst) {
            self.checked(&Verdict::Unchecked("PAM is still at an earlier check".into()), cx);
            return;
        }
        let busy = Busy(Arc::clone(&self.pam_busy));
        let checked = cx.background_spawn(async move {
            let _busy = busy;
            pam::check(&user, password)
        });
        self.check = Some(cx.spawn(async move |this, cx| {
            let mut too_long = cx.background_executor().timer(CHECK_TIMEOUT).fuse();
            let verdict = futures::select_biased! {
                verdict = checked.fuse() => verdict,
                () = too_long => Verdict::Unchecked(format!("PAM didn't answer in {} s", CHECK_TIMEOUT.as_secs())),
            };
            let _ = this.update(cx, |lock, cx| lock.checked(&verdict, cx));
        }));
    }

    fn checked(&mut self, verdict: &Verdict, cx: &mut Context<Self>) {
        self.check = None;
        match verdict.refusal() {
            None => self.unlock_after_fade(cx),
            Some((shown, logged)) => {
                eprintln!("WARN nova-ui: lock screen: {logged}");
                self.typed.clear();
                self.set_failed(true);
                self.message = Some(shown);
                self.shaken = Some(Instant::now());
            }
        }
        cx.notify();
    }

    fn unlock_after_fade(&mut self, cx: &mut Context<Self>) {
        self.unlocking = Some(Instant::now());
        self.release = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DURATION_UNLOCK).await;
            let _ = this.update(cx, |lock, cx| lock.unlock(cx));
        }));
    }

    fn unlock(&mut self, cx: &mut Context<Self>) {
        if !self.held {
            return;
        }
        cx.unlock_session();
        eprintln!("INFO nova-ui: session unlocked");
        self.report_unlocked(cx);
        self.let_go(cx);
    }

    fn sync_windows(&mut self, cx: &mut Context<Self>) {
        if !self.held {
            return;
        }
        let Some(screens) = Stage::screens(cx) else { return };
        let here = screens.read(cx).list().to_vec();
        let mut windows = std::mem::take(&mut self.windows);
        windows.retain(|(screen, window)| {
            let kept = here.iter().any(|kept| kept.id == *screen) && window.update(cx, |_, _, _| ()).is_ok();
            if !kept {
                let _ = window.update(cx, |_, window, _| window.remove_window());
            }
            kept
        });
        self.windows = windows;
        let waits = |id: &DisplayId| self.opening.contains(id) || self.unopened.contains(id);
        let new: Vec<Screen> = here
            .iter()
            .filter(|screen| !self.windows.iter().any(|(id, _)| *id == screen.id) && !waits(&screen.id))
            .cloned()
            .collect();
        if new.is_empty() {
            return;
        }
        self.opening.extend(new.iter().map(|screen| screen.id));
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            let opened: Vec<_> =
                new.iter().map(|screen| (screen.id, LockSurface::open(screen, this.clone(), cx))).collect();
            let _ = this.update(cx, |lock, cx| lock.opened(opened, cx));
        });
    }

    fn opened(&mut self, opened: Vec<(DisplayId, Option<WindowHandle<LockSurface>>)>, cx: &mut Context<Self>) {
        for (screen, window) in opened {
            self.opening.retain(|id| *id != screen);
            match window {
                Some(window) if self.held => self.windows.push((screen, window)),
                Some(window) => {
                    let _ = window.update(cx, |_, window, _| window.remove_window());
                }
                None => self.unopened.push(screen),
            }
        }
        self.sync_windows(cx);
    }

    fn close_windows(&mut self, cx: &mut App) {
        for (_, window) in self.windows.drain(..) {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
    }

    pub fn prompt_on(&self, screen: DisplayId, cx: &App) -> Option<Prompt> {
        let desktop = self.desktop.read(cx).get();
        let focused = desktop.and_then(|desktop| desktop.outputs.iter().find(|output| output.focused));
        let screens = Stage::screens(cx)?;
        let prompting = screens.read(cx).for_output(focused.map(|output| output.name.as_str()))?.id;
        (self.held && prompting == screen).then(|| Prompt {
            user: self.user.clone(),
            typed: self.typed.len(),
            checking: self.check.is_some(),
            failed: self.failed,
            ring_changed: self.ring_changed,
            message: self.message,
            shaken: self.shaken,
            unlocking: self.unlocking,
        })
    }
}

struct Busy(Arc<AtomicBool>);

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn user_name() -> String {
    USER_VARS.iter().find_map(|name| std::env::var(name).ok()).unwrap_or_default()
}
