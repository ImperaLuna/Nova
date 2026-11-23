//! The screens there are right now. GPUI has no event for a screen that
//! comes or goes, so [`Screens`] looks once in a while, on a timer that
//! asks for no frame, and notifies when the list changed. The stage opens
//! and closes its windows by it, and a feature with windows of its own (a
//! pop-up, the volume capsule) observes it instead of looking itself.

use crate::tokens::{FALLBACK_REFRESH, SCREEN_POLL};
use gpui::{App, AppContext, Context, DisplayId, Entity, Pixels, PlatformDisplay, SharedString, Size};
use std::rc::Rc;
use std::time::Duration;

/// One screen, as the stage and the views on it lay themselves out by it.
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    /// What GPUI calls it: a window opens on a screen by this.
    pub id: DisplayId,
    /// What the compositor calls it, like `DP-1`: novad's messages name an
    /// output this way (a workspace's, `nova toggle`'s). Empty when the
    /// compositor gave none.
    pub name: SharedString,
    pub size: Size<Pixels>,
    /// The time between two of its frames.
    pub refresh: Duration,
}

impl Screen {
    fn of(display: &Rc<dyn PlatformDisplay>) -> Self {
        Self {
            id: display.id(),
            name: display.name().unwrap_or_default().into(),
            size: display.bounds().size,
            refresh: display.refresh_interval().unwrap_or(FALLBACK_REFRESH),
        }
    }
}

/// Every screen, sorted by name.
pub struct Screens {
    list: Vec<Screen>,
}

impl Screens {
    /// Starts looking. The entity notifies when a screen came, went or
    /// changed its size.
    pub(crate) fn follow(cx: &mut App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            cx.spawn(async move |screens, cx| {
                // Until nothing follows the screens any more.
                let mut followed = true;
                while followed {
                    cx.background_executor().timer(SCREEN_POLL).await;
                    followed = screens.update(cx, Self::look_again).is_ok();
                }
            })
            .detach();
            Self { list: Self::look(cx) }
        })
    }

    fn look_again(&mut self, cx: &mut Context<Self>) {
        let now = Self::look(cx);
        if now != self.list {
            self.list = now;
            cx.notify();
        }
    }

    fn look(cx: &App) -> Vec<Screen> {
        // GPUI lists them in no order that lasts.
        let mut screens: Vec<Screen> = cx.displays().iter().map(Screen::of).collect();
        screens.sort_by(|a, b| a.name.cmp(&b.name));
        screens
    }

    pub fn list(&self) -> &[Screen] {
        &self.list
    }

    pub fn get(&self, id: DisplayId) -> Option<&Screen> {
        self.list.iter().find(|screen| screen.id == id)
    }

    /// The screen novad means by `output` (the focused output's name): that
    /// one, or the first when novad doesn't know or names one that isn't
    /// here.
    pub fn for_output(&self, output: Option<&str>) -> Option<&Screen> {
        let named = output.and_then(|name| self.list.iter().find(|screen| screen.name.as_ref() == name));
        named.or(self.list.first())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{px, size};

    fn screen(id: u64, name: &'static str) -> Screen {
        Screen {
            id: DisplayId::new(id),
            name: name.into(),
            size: size(px(1920.0), px(1080.0)),
            refresh: FALLBACK_REFRESH,
        }
    }

    #[test]
    fn nova_toggle_goes_to_the_focused_output_or_the_first_screen() {
        let screens = Screens { list: vec![screen(1, "DP-1"), screen(2, "DP-2")] };
        assert_eq!(screens.for_output(Some("DP-2")).map(|screen| screen.id), Some(DisplayId::new(2)));
        assert_eq!(screens.for_output(None).map(|screen| screen.id), Some(DisplayId::new(1)));
        assert_eq!(screens.for_output(Some("HDMI-A-1")).map(|screen| screen.id), Some(DisplayId::new(1)));
        assert_eq!(Screens { list: Vec::new() }.for_output(None), None);
    }
}
