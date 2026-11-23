//! The lock: it asks the compositor for the session lock when novad says
//! so, keeps a lock surface on every screen while it holds, takes the keys
//! typed on any of them, checks the password and unlocks. `LockScreen.qml`,
//! `Authenticator.qml` and `LockService.qml` in today's shell.
//!
//! The password typed so far lives here, shared by every screen, so
//! whichever surface the compositor sends the keys to types into it. Each
//! surface observes this and draws from a copy of what it shows
//! ([`LockScreen::prompt_on`]); only the screen in use shows the prompt.
//!
//! While the session is locked this keeps itself alive ([`Held`]): the
//! stage drops an overlay when its feature goes, and a lock dropped with
//! the feature turned off would leave the screens covered with nothing to
//! type into.

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

/// Where the user's name comes from, as in today's shell.
const USER_VARS: [&str; 2] = ["USER", "LOGNAME"];
/// PAM gets this long to answer; a check that hangs must not lock the user
/// out with a spinner.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// The lock screen while the session is locked, so it outlives its
/// feature being turned off meanwhile.
#[derive(Default)]
struct Held(Option<Entity<LockScreen>>);

impl Global for Held {}

/// What one screen's prompt shows. Plain values a surface keeps and draws
/// from: it never reads the lock while drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    pub user: SharedString,
    /// Characters typed so far; their text never reaches a surface.
    pub typed: usize,
    pub checking: bool,
    /// The last try was wrong: the field rings in the error color.
    pub failed: bool,
    /// When `failed` last changed, for the ring's change of color.
    pub ring_changed: Option<Instant>,
    pub message: Option<&'static str>,
    /// When the last wrong password shook the field.
    pub shaken: Option<Instant>,
    /// The password checked out then: the prompt fades.
    pub unlocking: Option<Instant>,
}

pub struct LockScreen {
    /// This holds the session lock: asked for, or confirmed.
    held: bool,
    /// The compositor confirmed the lock this holds.
    confirmed: bool,
    /// novad wasn't reached when the session was unlocked, so it still
    /// says locked.
    unreported: bool,
    user: SharedString,
    typed: Secret,
    /// The check under way.
    check: Option<Task<()>>,
    /// PAM is still at a check, maybe one given up on: it gets no second
    /// one meanwhile, so checks that hang can't pile up.
    pam_busy: Arc<AtomicBool>,
    failed: bool,
    ring_changed: Option<Instant>,
    message: Option<&'static str>,
    shaken: Option<Instant>,
    unlocking: Option<Instant>,
    /// Lets go of the lock once the prompt has faded.
    release: Option<Task<()>>,
    windows: Vec<(DisplayId, WindowHandle<LockSurface>)>,
    /// Screens whose window is about to open, and those whose window
    /// couldn't open (tried again when the screens change).
    opening: Vec<DisplayId>,
    unopened: Vec<DisplayId>,
    desktop: Entity<Live<Desktop>>,
    _followed: Vec<Subscription>,
}

impl LockScreen {
    /// The lock screen: the one holding the session lock, else a new one.
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
            // novad sends the state on every connection too, so a UI that
            // restarted while locked locks again.
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
                // The prompt moves with the focus.
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

    /// novad says the session is locked.
    fn asked(&mut self, cx: &mut Context<Self>) {
        if self.unreported && !self.held {
            // Its word is old: the session was unlocked while it couldn't
            // be reached.
            self.report_unlocked(cx);
            return;
        }
        self.lock(cx);
    }

    fn lock(&mut self, cx: &mut Context<Self>) {
        if self.held {
            if self.unlocking.is_some() {
                // Asked while the prompt fades after the right password:
                // the lock stays, with an empty prompt.
                self.reset();
                cx.notify();
            }
            // Asked again while locked (a novad that restarted lost who
            // waits for the screens to be covered): they are.
            if cx.session_lock_state() == SessionLockState::Locked {
                Daemon::send(Command::ReportLocked, cx);
            }
            return;
        }
        if self.user.is_empty() {
            // PAM would have nobody to check a password for.
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

    /// The compositor confirmed the lock, or refused or ended it.
    fn compositor_answered(&mut self, cx: &mut Context<Self>) {
        if !self.held {
            return;
        }
        match cx.session_lock_state() {
            SessionLockState::Locked => {
                eprintln!("INFO nova-ui: session locked");
                self.confirmed = true;
                // `nova lock` can answer: the screens are covered.
                Daemon::send(Command::ReportLocked, cx);
            }
            // A lock it had confirmed and then ended leaves the session
            // open, and novad has to know to lock it again.
            SessionLockState::Unlocked if self.confirmed => {
                eprintln!("WARN nova-ui: the compositor ended the session lock");
                self.report_unlocked(cx);
                self.let_go(cx);
            }
            // A lock it refused: another lock screen holds the session, or
            // the compositor won't let a new one take over from one that
            // stopped. novad keeps its word and asks again next time.
            SessionLockState::Unlocked => {
                eprintln!(
                    "ERROR nova-ui: the compositor refused the session lock. On Hyprland a lock screen that \
                     restarted can lock again only with {HYPRLAND_LOCK_RESTORE} = true, which Nova's Hyprland \
                     rules set; `nova doctor` checks it"
                );
                // Hyprland never makes the lock surfaces asked for on a lock
                // it refused (SessionLock.cpp, "getLockSurface after it's
                // inert"), so closing their windows destroys objects it
                // doesn't know: a protocol error that ends nova-ui, and the
                // bar with it, on every restart. Their windows stay open,
                // never shown, until nova-ui exits. Remove this once the
                // GPUI fork leaves a refused lock's surfaces alone itself
                // (IMP-454).
                self.windows.clear();
                self.let_go(cx);
            }
            SessionLockState::Pending => {}
        }
    }

    /// The lock is no longer this one's: no windows, nothing typed.
    fn let_go(&mut self, cx: &mut Context<Self>) {
        (self.held, self.confirmed) = (false, false);
        self.reset();
        self.close_windows(cx);
        // Dropped with its feature off, once this update ends.
        cx.set_global(Held(None));
        cx.notify();
    }

    /// Tells novad the session is unlocked, and remembers when it couldn't
    /// be told: novad says locked on its next connection, and the session
    /// would lock itself again.
    fn report_unlocked(&mut self, cx: &mut Context<Self>) {
        self.unreported = false;
        let reported = Daemon::call::<()>(Command::ReportUnlocked, cx);
        cx.spawn(async move |this, cx| {
            if let Err(refusal) = reported.await {
                eprintln!("WARN nova-ui: report_unlocked failed: {refusal}");
                // Only with the link down: its next connection starts with
                // novad's old word, which is what `asked` takes back. Any
                // other "locked" is a lock asked for.
                let link_down = matches!(refusal, Refusal::NotConnected | Refusal::WentAway);
                let _ = this.update(cx, |lock, _| lock.unreported = link_down);
            }
        })
        .detach();
    }

    fn reset(&mut self) {
        // A window that couldn't open gets another try with each lock.
        self.unopened.clear();
        self.typed.clear();
        (self.check, self.release) = (None, None);
        (self.failed, self.message) = (false, None);
        (self.ring_changed, self.shaken, self.unlocking) = (None, None, None);
    }

    /// A key pressed on any screen's surface.
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
        // Free again when the check ends, or never starts (its task
        // dropped first).
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

    /// The password checked out: the prompt fades, then the lock lets go.
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
        // The compositor first, then the surfaces it no longer shows.
        cx.unlock_session();
        eprintln!("INFO nova-ui: session unlocked");
        self.report_unlocked(cx);
        self.let_go(cx);
    }

    /// While the lock holds: a window on each screen that has none, and
    /// none on a screen that left. The session stays locked with no screen
    /// at all; the next one to come gets its window.
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
        // Opened after this update: a new window draws at once, and its
        // view reads the lock as it is made.
        self.opening.extend(new.iter().map(|screen| screen.id));
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            let opened: Vec<_> =
                new.iter().map(|screen| (screen.id, LockSurface::open(screen, this.clone(), cx))).collect();
            let _ = this.update(cx, |lock, cx| lock.opened(opened, cx));
        });
    }

    /// The windows a deferred open made, by screen; then the screens are
    /// looked at again, in case they changed meanwhile.
    fn opened(&mut self, opened: Vec<(DisplayId, Option<WindowHandle<LockSurface>>)>, cx: &mut Context<Self>) {
        for (screen, window) in opened {
            self.opening.retain(|id| *id != screen);
            match window {
                Some(window) if self.held => self.windows.push((screen, window)),
                // Unlocked while it opened.
                Some(window) => {
                    let _ = window.update(cx, |_, window, _| window.remove_window());
                }
                None => self.unopened.push(screen),
            }
        }
        self.sync_windows(cx);
    }

    /// Closes every lock surface. This never unlocks.
    fn close_windows(&mut self, cx: &mut App) {
        for (_, window) in self.windows.drain(..) {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// What `screen`'s surface shows over the backdrop: the prompt on the
    /// focused screen (the first one when novad doesn't know which), and
    /// nothing on the others.
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

/// Marks PAM free when dropped.
struct Busy(Arc<AtomicBool>);

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn user_name() -> String {
    USER_VARS.iter().find_map(|name| std::env::var(name).ok()).unwrap_or_default()
}
