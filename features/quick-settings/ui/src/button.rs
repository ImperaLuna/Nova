use crate::NAME;
use crate::logic::{Badge, badge};
use crate::tokens::{BADGE_ICON, BADGE_OFFSET, BADGE_PAD, BADGE_QUIET_OPACITY, BADGE_RING, BADGE_SIZE};
use gpui::{AnyElement, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div};
use nova_look::motion::ease;
use nova_look::tokens::{DURATION_COLOR, DURATION_FAST, ENTER_SCALE, FONT_MONO, FONT_SIZE_MICRO, WEIGHT_BADGE};
use nova_look::{Glyph, Icon, PillButton, Theme, fade};
use nova_notifications_api::{Event, TOPIC};
use nova_stage::{Screen, Stage};
use nova_ui_client::Live;

type Unread = (usize, bool);

pub struct Button {
    screen: Screen,
    notifications: Entity<Live<Unread>>,
    last_badge: Option<Badge>,
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
        Self { screen: screen.clone(), notifications, last_badge: None, _followed: followed }
    }
}

impl Render for Button {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = self.screen.id;
        let open = Stage::is_open(NAME, screen, cx);
        let (unread, quiet) = self.notifications.read(cx).get().copied().unwrap_or_default();
        let button = PillButton::new("quick-settings", Glyph::TUNE)
            .picked(open)
            .on_click(move |_, _, cx| Stage::toggle(NAME, screen, cx));
        let shown = badge(unread, quiet, open);
        if shown.is_some() {
            self.last_badge.clone_from(&shown);
        }
        let target = if shown.is_some() { 1.0 } else { 0.0 };
        let opacity = fade("quick-settings-badge-shown", target, DURATION_FAST, window, cx);
        let grown = ease(fade("quick-settings-badge-grown", target, DURATION_COLOR, window, cx));
        match self.last_badge.as_ref().filter(|_| opacity > 0.0) {
            Some(drawn) => button.child(badge_element(drawn, opacity, grown, cx)),
            None => button,
        }
    }
}

fn badge_element(shown: &Badge, opacity: f32, grown: f32, cx: &Context<Button>) -> AnyElement {
    let theme = Theme::of(cx);
    let scale = ENTER_SCALE + (1.0 - ENTER_SCALE) * grown;
    let off_center = BADGE_SIZE * (1.0 - scale) / 2.0;
    let fill = match shown {
        Badge::Quiet => theme.fg.alpha(BADGE_QUIET_OPACITY),
        Badge::Count(_) => theme.primary,
    };
    let inside = match shown {
        Badge::Quiet => Icon::new(Glyph::BELL_OFF).size(BADGE_ICON * scale).color(theme.on_accent).into_any_element(),
        Badge::Count(count) => div()
            .font_family(FONT_MONO)
            .text_size(FONT_SIZE_MICRO * scale)
            .font_weight(WEIGHT_BADGE)
            .text_color(theme.on_accent)
            .child(count.clone())
            .into_any_element(),
    };
    div()
        .absolute()
        .top(off_center - BADGE_OFFSET)
        .right(off_center - BADGE_OFFSET)
        .h(BADGE_SIZE * scale)
        .min_w(BADGE_SIZE * scale)
        .px((BADGE_PAD - BADGE_RING) * scale)
        .rounded_full()
        .opacity(opacity)
        .bg(fill)
        .border(BADGE_RING * scale)
        .border_color(theme.bg)
        .flex()
        .items_center()
        .justify_center()
        .child(inside)
        .into_any_element()
}
