//! Quick settings' button on the right pill (`quick-settings.button` in
//! `right-pill.buttons`), and in the same place on the open drawer: opens and
//! closes the drawer on its screen, picked while it is open. Its badge says how
//! many notifications are unread, or shows a grey crossed-out bell during do
//! not disturb; none while open.

use crate::NAME;
use crate::logic::{Badge, badge};
use crate::tokens::{BADGE_ICON, BADGE_OFFSET, BADGE_PAD, BADGE_QUIET_OPACITY, BADGE_RING, BADGE_SIZE};
use gpui::{AnyElement, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div};
use nova_look::tokens::{FONT_MONO, FONT_SIZE_MICRO, WEIGHT_BADGE};
use nova_look::{Glyph, Icon, PillButton, Theme};
use nova_notifications_api::{Event, TOPIC};
use nova_stage::{Screen, Stage};
use nova_ui_client::Live;

/// What the badge reads from notifications: how many are unread, and do
/// not disturb.
type Unread = (usize, bool);

pub struct Button {
    screen: Screen,
    notifications: Entity<Live<Unread>>,
    _followed: Vec<Subscription>,
}

impl Button {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let notifications = Live::follow(TOPIC, cx, |event| match event {
            Event::Notifications(list) => {
                Some((list.items.iter().filter(|item| item.unread).count(), list.do_not_disturb))
            }
            Event::NotificationPopup(_) => None,
        });
        let mut followed = vec![cx.observe(&notifications, |_, _, cx| cx.notify())];
        followed.extend(Stage::observe_panels(cx, |_, cx| cx.notify()));
        Self { screen: screen.clone(), notifications, _followed: followed }
    }
}

impl Render for Button {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = self.screen.id;
        let open = Stage::is_open(NAME, screen, cx);
        let (unread, quiet) = self.notifications.read(cx).get().copied().unwrap_or_default();
        let button = PillButton::new("quick-settings", Glyph::TUNE)
            .picked(open)
            .on_click(move |_, _, cx| Stage::toggle(NAME, screen, cx));
        match badge(unread, quiet, open) {
            Some(shown) => button.child(badge_element(&shown, cx)),
            None => button,
        }
    }
}

/// The badge on the button's top right corner, ringed in the background.
fn badge_element(shown: &Badge, cx: &Context<Button>) -> AnyElement {
    let theme = Theme::of(cx);
    let fill = match shown {
        Badge::Quiet => theme.fg.alpha(BADGE_QUIET_OPACITY),
        Badge::Count(_) => theme.primary,
    };
    let inside = match shown {
        Badge::Quiet => Icon::new(Glyph::BELL_OFF).size(BADGE_ICON).color(theme.on_accent).into_any_element(),
        Badge::Count(count) => div()
            .font_family(FONT_MONO)
            .text_size(FONT_SIZE_MICRO)
            .font_weight(WEIGHT_BADGE)
            .text_color(theme.on_accent)
            .child(count.clone())
            .into_any_element(),
    };
    div()
        .absolute()
        .top(-BADGE_OFFSET)
        .right(-BADGE_OFFSET)
        .h(BADGE_SIZE)
        .min_w(BADGE_SIZE)
        .px(BADGE_PAD - BADGE_RING)
        .rounded_full()
        .bg(fill)
        .border(BADGE_RING)
        .border_color(theme.bg)
        .flex()
        .items_center()
        .justify_center()
        .child(inside)
        .into_any_element()
}
