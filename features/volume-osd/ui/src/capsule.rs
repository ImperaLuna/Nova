use crate::logic::{self, Output};
use crate::motion::LiquidMotion;
use crate::tokens::{HOLD, SWELL};
use crate::window::CapsuleWindow;
use gpui::{App, AppContext, Context, DisplayId, Entity, Subscription, Task, WindowHandle};
use nova_desktop_info::{Desktop, Event as DesktopEvent};
use nova_look::motion::Eased;
use nova_look::tokens::DURATION_COLOR;
use nova_sound_api::{Command as SoundCommand, Event as SoundEvent, Kind, Sound};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Daemon, Live};
use nova_volume_osd_api::{Command as KeyCommand, Event as KeyEvent, VolumeKey};
use std::time::Instant;

const QUICK_SETTINGS: &str = "quick-settings";

pub struct Capsule {
    _sound: Entity<Live<Sound>>,
    desktop: Entity<Live<Desktop>>,
    output: Option<Output>,
    seen: Option<Output>,
    target: Option<DisplayId>,
    motion: LiquidMotion,
    wants_out: bool,
    swell: Eased,
    muted: Eased,
    stepped: Option<Instant>,
    held: bool,
    hold: Option<Task<()>>,
    windows: Vec<(DisplayId, WindowHandle<CapsuleWindow>)>,
    opening: Vec<DisplayId>,
    failed: Vec<DisplayId>,
    _followed: Vec<Subscription>,
}

impl Capsule {
    pub fn start(cx: &mut App) -> Entity<Self> {
        let capsule = cx.new(|cx: &mut Context<Self>| {
            let sound = Live::follow(nova_sound_api::TOPIC, cx, |SoundEvent::Sound(sound)| Some(sound));
            let desktop = Live::follow(nova_desktop_info::TOPIC, cx, |DesktopEvent::Compositor(desktop)| Some(desktop));
            let this = cx.weak_entity();
            let keys = Daemon::on_event(nova_volume_osd_api::TOPIC, cx, move |KeyEvent::Volume { .. }, cx| {
                let _ = this.update(cx, |capsule, cx| capsule.reveal(cx));
            });
            let mut followed = vec![
                keys,
                cx.observe(&sound, Self::follow_sound),
                cx.on_release(|capsule: &mut Self, cx| capsule.close_windows(cx)),
            ];
            if let Some(screens) = Stage::screens(cx) {
                followed.push(cx.observe(&screens, |capsule: &mut Self, _, cx| {
                    capsule.failed.clear();
                    capsule.sync_windows(cx);
                }));
            }
            let this = cx.weak_entity();
            if let Some(open) = Stage::observe_open(cx, move |cx| {
                let _ = this.update(cx, |capsule, cx| capsule.follow_quick_settings(cx));
            }) {
                followed.push(open);
            }
            let output = logic::output(sound.read(cx).get());
            let now = Instant::now();
            Self {
                _sound: sound,
                desktop,
                output,
                seen: output,
                target: None,
                motion: LiquidMotion::at(logic::level(output), still(output)),
                wants_out: false,
                swell: Eased::lasting(swell(output), DURATION_COLOR, now),
                muted: Eased::lasting(muted(output), DURATION_COLOR, now),
                stepped: None,
                held: false,
                hold: None,
                windows: Vec::new(),
                opening: Vec::new(),
                failed: Vec::new(),
                _followed: followed,
            }
        });
        capsule.update(cx, |capsule, cx| capsule.sync_windows(cx));
        capsule
    }

    fn sync_windows(&mut self, cx: &mut Context<Self>) {
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
        if self.target.is_some_and(|target| !here.iter().any(|screen| screen.id == target)) {
            self.hide_now(cx);
        }
        let new: Vec<Screen> = here
            .iter()
            .filter(|screen| {
                !self.windows.iter().any(|(id, _)| *id == screen.id)
                    && !self.opening.contains(&screen.id)
                    && !self.failed.contains(&screen.id)
            })
            .cloned()
            .collect();
        if new.is_empty() {
            return;
        }
        self.opening.extend(new.iter().map(|screen| screen.id));
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            let opened: Vec<_> =
                new.iter().map(|screen| (screen.id, CapsuleWindow::open(screen, this.clone(), cx))).collect();
            let _ = this.update(cx, |capsule, cx| capsule.opened(opened, cx));
        });
    }

    fn opened(&mut self, opened: Vec<(DisplayId, Option<WindowHandle<CapsuleWindow>>)>, cx: &mut Context<Self>) {
        for (screen, window) in opened {
            self.opening.retain(|id| *id != screen);
            match window {
                Some(window) => self.windows.push((screen, window)),
                None => self.failed.push(screen),
            }
        }
        self.sync_windows(cx);
    }

    fn close_windows(&mut self, cx: &mut App) {
        for (_, window) in self.windows.drain(..) {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
    }

    fn follow_sound(&mut self, sound: Entity<Live<Sound>>, cx: &mut Context<Self>) {
        let now = logic::output(sound.read(cx).get());
        if now == self.output {
            return;
        }
        let at = Instant::now();
        self.output = now;
        self.motion.level_target = logic::level(now);
        self.motion.still = still(now);
        self.swell.set_target(swell(now), at);
        self.muted.set_target(muted(now), at);
        let changed = logic::is_change(self.seen, now);
        if now.is_some() {
            self.seen = now;
        }
        if changed {
            self.reveal(cx);
        }
        cx.notify();
    }

    fn reveal(&mut self, cx: &mut Context<Self>) {
        let target = if self.motion.shown() { self.target } else { self.focused_screen(cx) };
        let Some(target) = target else { return };
        if Stage::is_open(QUICK_SETTINGS, target, cx) || !self.has_window(target) {
            return;
        }
        if self.target != Some(target) {
            self.stepped = None;
        }
        self.target = Some(target);
        if self.motion.open_target > 0.0 {
            self.motion.open_target = 1.0;
        } else {
            self.wants_out = true;
        }
        self.restart_hold(cx);
        cx.notify();
    }

    fn has_window(&self, screen: DisplayId) -> bool {
        self.windows.iter().any(|(id, _)| *id == screen) || self.opening.contains(&screen)
    }

    fn focused_screen(&self, cx: &App) -> Option<DisplayId> {
        let desktop = self.desktop.read(cx).get();
        let output = desktop.and_then(|desktop| desktop.outputs.iter().find(|output| output.focused));
        let screens = Stage::screens(cx)?;
        screens.read(cx).for_output(output.map(|output| output.name.as_str())).map(|screen| screen.id)
    }

    fn restart_hold(&mut self, cx: &mut Context<Self>) {
        self.hold = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOLD).await;
            let _ = this.update(cx, |capsule, cx| {
                if !capsule.held {
                    capsule.go_in(cx);
                }
            });
        }));
    }

    fn go_in(&mut self, cx: &mut Context<Self>) {
        self.hold = None;
        self.wants_out = false;
        self.motion.open_target = 0.0;
        cx.notify();
    }

    fn hide_now(&mut self, cx: &mut Context<Self>) {
        self.go_in(cx);
        let level = self.motion.level_target;
        self.motion = LiquidMotion::at(level, self.motion.still);
        self.target = None;
    }

    fn follow_quick_settings(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target else { return };
        if (self.motion.shown() || self.wants_out) && Stage::is_open(QUICK_SETTINGS, target, cx) {
            self.go_in(cx);
        }
    }

    pub fn set_held(&mut self, held: bool, cx: &mut Context<Self>) {
        if held == self.held {
            return;
        }
        self.held = held;
        if !held && self.motion.open_target > 0.0 {
            self.restart_hold(cx);
        }
    }

    pub fn shows_on(&self, screen: DisplayId) -> bool {
        self.target == Some(screen) && (self.wants_out || self.motion.shown())
    }

    pub fn wants_out(&self) -> bool {
        self.wants_out
    }

    pub fn go_out(&mut self) {
        if self.wants_out {
            self.wants_out = false;
            self.motion.open_target = 1.0;
        }
    }

    pub fn step(&mut self, now: Instant) {
        let elapsed = self.stepped.map_or_else(Default::default, |last| now.saturating_duration_since(last));
        self.stepped = self.motion.shown().then_some(now);
        self.motion.advance(elapsed);
    }

    pub fn motion(&self) -> &LiquidMotion {
        &self.motion
    }

    pub fn output(&self) -> Option<Output> {
        self.output
    }

    pub fn swell(&self, now: Instant) -> f32 {
        self.swell.value(now)
    }

    pub fn muted(&self, now: Instant) -> f32 {
        self.muted.value(now)
    }

    pub fn scroll(&mut self, notches: i32, cx: &mut Context<Self>) {
        let key = if notches > 0 { VolumeKey::Up } else { VolumeKey::Down };
        for _ in 0..notches.unsigned_abs() {
            Daemon::send(KeyCommand::Volume { key }, cx);
        }
    }

    pub fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        if let Some(output) = self.output.filter(|output| output.volume != volume) {
            Daemon::send(SoundCommand::SetSoundLevel { of: Kind::Output, id: output.id, volume }, cx);
        }
    }

    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        if let Some(output) = self.output {
            Daemon::send(SoundCommand::SetSoundMuted { of: Kind::Output, id: output.id, muted: !output.muted }, cx);
        }
    }
}

fn still(output: Option<Output>) -> bool {
    output.is_none_or(|output| output.muted)
}

fn swell(output: Option<Output>) -> f32 {
    if still(output) { 0.0 } else { SWELL }
}

fn muted(output: Option<Output>) -> f32 {
    if output.is_some_and(|output| output.muted) { 1.0 } else { 0.0 }
}
