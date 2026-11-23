use crate::deck::ByScreen;
use crate::draw::{device_size, picking_mark};
use crate::preview::Preview as Picture;
use gpui::{
    App, AppContext, Context, Entity, Global, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    WindowHandle, div,
    layer_shell::{Anchor, Layer},
};
use nova_look::motion::Eased;
use nova_look::tokens::{DURATION_FADE, DURATION_FAST, DURATION_PANEL};
use nova_look::{LookStyled, ShellWindow, set_blur_region};
use nova_stage::{FrameLog, Screen, Stage};
use std::time::Instant;

const NAMESPACE: &str = "nova-wallpaper-preview";

pub struct Preview {
    pub paths: ByScreen,
    pub choosing_for: Option<String>,
    pub open: bool,
}

#[derive(Default)]
pub struct Previewed {
    pub from: Option<String>,
    pub paths: ByScreen,
    pub choosing_for: Option<String>,
    pub shown: bool,
    windows: Vec<WindowHandle<PreviewWindow>>,
    opening: bool,
}

struct Shared(Entity<Previewed>);

impl Global for Shared {}

impl Previewed {
    pub fn get(cx: &mut App) -> Entity<Self> {
        if let Some(shared) = cx.try_global::<Shared>() {
            return shared.0.clone();
        }
        let previewed = cx.new(|_| Self::default());
        cx.set_global(Shared(previewed.clone()));
        previewed
    }

    pub fn set(&mut self, here: &str, preview: Preview, cx: &mut Context<Self>) {
        let Preview { paths, choosing_for, open } = preview;
        let ours = self.from.as_deref() == Some(here);
        if !(open || ours || self.from.is_none()) {
            return;
        }
        if !ours {
            self.close_windows(cx);
        }
        self.from = Some(here.to_owned());
        self.paths = paths;
        self.choosing_for = choosing_for;
        self.shown = open;
        if open {
            self.open_windows_once_settled(cx);
        }
        cx.notify();
    }

    pub fn clear(&mut self, here: &str, cx: &mut Context<Self>) {
        if self.from.as_deref() != Some(here) {
            return;
        }
        self.from = None;
        self.paths.clear();
        self.choosing_for = None;
        self.shown = false;
        self.close_windows(cx);
        cx.notify();
    }

    fn close_windows(&mut self, cx: &mut App) {
        for window in self.windows.drain(..) {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
    }

    fn open_windows_once_settled(&mut self, cx: &mut Context<Self>) {
        if self.opening || !self.windows.is_empty() {
            return;
        }
        self.opening = true;
        cx.spawn(async move |previewed, cx| {
            cx.background_executor().timer(DURATION_PANEL + DURATION_FADE).await;
            let wanted = previewed.update(cx, |previewed, cx| {
                previewed.opening = false;
                if previewed.shown { previewed.others(cx) } else { Vec::new() }
            });
            let (Ok(screens), Some(shared)) = (wanted, previewed.upgrade()) else { return };
            for screen in screens {
                let window = cx.update(|cx| PreviewWindow::open(&screen, shared.clone(), cx));
                let _ = previewed.update(cx, |previewed, _| previewed.windows.extend(window));
            }
        })
        .detach();
    }

    fn others(&self, cx: &App) -> Vec<Screen> {
        let Some(screens) = Stage::screens(cx) else { return Vec::new() };
        screens
            .read(cx)
            .list()
            .iter()
            .filter(|screen| self.from.as_deref() != Some(screen.name.as_ref()))
            .cloned()
            .collect()
    }
}

pub struct PreviewWindow {
    screen: Screen,
    previewed: Entity<Previewed>,
    preview: Picture,
    shown: Eased,
    picking: Eased,
    frames: Option<FrameLog>,
    blur_cleared: bool,
    _followed: Subscription,
}

impl PreviewWindow {
    fn open(screen: &Screen, previewed: Entity<Previewed>, cx: &mut App) -> Option<WindowHandle<Self>> {
        let mut shell = ShellWindow::new(screen.id, screen.size);
        shell.layer = Layer::Overlay;
        shell.anchor = Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT;
        shell.namespace = NAMESPACE;
        let screen = screen.clone();
        let frames = Stage::logs_frames(cx)
            .then(|| FrameLog::new(format!("{}/wallpaper-preview", screen.name), screen.refresh, Instant::now()));
        let opened = cx.open_window(shell.options(), move |_, cx| {
            cx.new(|cx: &mut Context<Self>| {
                let now = Instant::now();
                let followed = cx.observe(&previewed, |_, _, cx| cx.notify());
                Self {
                    screen,
                    previewed,
                    preview: Picture::new(cx),
                    shown: Eased::lasting(0.0, DURATION_PANEL, now),
                    picking: Eased::lasting(0.0, DURATION_FAST, now),
                    frames,
                    blur_cleared: false,
                    _followed: followed,
                }
            })
        });
        match opened {
            Ok(window) => Some(window),
            Err(error) => {
                eprintln!("ERROR nova-ui: can't open the wallpaper preview: {error:#}");
                None
            }
        }
    }
}

impl Render for PreviewWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if !self.blur_cleared {
            set_blur_region(window, &[]);
            window.set_input_region(Some(&[]));
            self.blur_cleared = true;
        }
        let previewed = self.previewed.read(cx);
        let name = self.screen.name.as_ref();
        let path = previewed.paths.get(name).cloned().unwrap_or_default();
        let shown = previewed.shown;
        let picking = shown && previewed.choosing_for.as_deref() == Some(name);
        self.shown.set_target(if shown { 1.0 } else { 0.0 }, now);
        self.picking.set_target(if picking { 1.0 } else { 0.0 }, now);
        let pixels = device_size(self.screen.size.width, self.screen.size.height, window);
        self.preview.show(&path, pixels, cx);
        let picture = self.preview.element(now, cx);
        self.preview.release(Some(window), cx);
        let moving = !(self.shown.settled(now) && self.picking.settled(now)) || self.preview.moving(now);
        if moving {
            window.request_animation_frame();
        }
        if let Some(frames) = &mut self.frames {
            for line in frames.frame(moving, now) {
                eprintln!("INFO nova-ui: {line}");
            }
        }
        div()
            .size_full()
            .body_text(cx)
            .child(div().absolute().inset_0().opacity(self.shown.value(now)).child(picture))
            .children(picking_mark(self.picking.value(now), cx))
    }
}
