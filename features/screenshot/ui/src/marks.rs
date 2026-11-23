//! The corner marks around a region being recorded, on its screen: just
//! outside it, so they aren't in the recording. `ui/capture/RecordingMarks.qml`
//! in today's shell.
//!
//! Its own window on each screen, on the overlay layer, taking no clicks,
//! no keyboard and no space: one pixel big while nothing is recorded there,
//! else from the screen's top left corner to the farthest mark.

use crate::capture::{Capture, on_screen};
use crate::pick::marks;
use crate::tokens::{MARK_GAP, MARK_LENGTH, MARK_WIDTH};
use gpui::{
    App, AppContext, Bounds, Context, IntoElement, ParentElement, Pixels, Render, Styled, Subscription, WeakEntity,
    Window, WindowHandle, div,
    layer_shell::{Anchor, Layer},
    px, size,
};
use nova_look::{ShellWindow, Theme, set_blur_region};
use nova_stage::Screen;

/// The marks' layer surfaces, for the compositor's rules.
const NAMESPACE: &str = "nova-recording";
/// A window with nothing recorded on its screen.
const RESTING: Pixels = px(1.0);

pub struct MarksWindow {
    screen: Screen,
    capture: WeakEntity<Capture>,
    /// The window's size now: what the marks need, or none while it rests.
    sized: Option<gpui::Size<Pixels>>,
    _followed: Option<Subscription>,
}

impl MarksWindow {
    pub fn open(screen: &Screen, capture: WeakEntity<Capture>, cx: &mut App) -> Option<WindowHandle<Self>> {
        let mut shell = ShellWindow::new(screen.id, size(RESTING, RESTING));
        shell.layer = Layer::Overlay;
        shell.anchor = Anchor::TOP | Anchor::LEFT;
        shell.namespace = NAMESPACE;
        let screen = screen.clone();
        let opened = cx.open_window(shell.options(), move |window, cx| {
            window.set_input_region(Some(&[]));
            set_blur_region(window, &[]);
            cx.new(|cx: &mut Context<Self>| {
                let followed = capture.upgrade().map(|capture| cx.observe(&capture, |_, _, cx| cx.notify()));
                Self { screen, capture, sized: None, _followed: followed }
            })
        });
        match opened {
            Ok(window) => Some(window),
            Err(error) => {
                eprintln!("ERROR nova-ui: can't open the recording marks' window: {error:#}");
                None
            }
        }
    }

    pub fn set_screen(&mut self, screen: &Screen, cx: &mut Context<Self>) {
        if self.screen != *screen {
            self.screen = screen.clone();
            cx.notify();
        }
    }

    /// The marks on this screen while a region of it is recorded.
    fn marks(&self, cx: &App) -> Option<[Bounds<Pixels>; 8]> {
        let capture = self.capture.upgrade()?;
        let recording = capture.read(cx).recording()?;
        let region = recording.region.filter(|_| recording.output == self.screen.name.as_ref())?;
        Some(marks(on_screen(region, &self.screen), MARK_GAP, MARK_LENGTH, MARK_WIDTH))
    }
}

impl Render for MarksWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let marks = self.marks(cx);
        // From the corner it is anchored to, to the farthest mark.
        let sized = marks.map(|marks| {
            let right = marks.iter().map(|mark| mark.origin.x + mark.size.width).fold(px(1.0), Pixels::max);
            let bottom = marks.iter().map(|mark| mark.origin.y + mark.size.height).fold(px(1.0), Pixels::max);
            size(right, bottom)
        });
        if sized != self.sized {
            self.sized = sized;
            window.resize(sized.unwrap_or(size(RESTING, RESTING)));
        }
        let error = Theme::of(cx).error;
        div().size_full().children(marks.into_iter().flatten().map(|mark| {
            div().absolute().left(mark.origin.x).top(mark.origin.y).w(mark.size.width).h(mark.size.height).bg(error)
        }))
    }
}
