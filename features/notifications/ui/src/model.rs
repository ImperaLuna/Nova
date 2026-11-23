//! Notifications' link to novad, shared by the pop-ups and the list: the
//! list, do not disturb and the muted apps, the pop-ups' arrivals, and the
//! commands. `ui/notifications/service/NotificationsService.qml` in today's
//! shell.

use gpui::{App, AppContext, Context, Entity, EventEmitter, Global, Subscription, WeakEntity};
use nova_notifications_api::{Command, Event, Notification, NotificationList, TOPIC};
use nova_ui_client::{Daemon, Live};

/// A notification to pop up.
pub struct Popped(pub Notification);

pub struct Model {
    list: Entity<Live<NotificationList>>,
    _popups: Subscription,
}

impl EventEmitter<Popped> for Model {}

/// The one model while anything holds it: the pop-ups and each screen's list.
#[derive(Default)]
struct Shared(Option<WeakEntity<Model>>);

impl Global for Shared {}

impl Model {
    /// The model, made on first use; it goes once nothing holds it.
    pub fn get(cx: &mut App) -> Entity<Self> {
        if let Some(model) = cx.default_global::<Shared>().0.as_ref().and_then(WeakEntity::upgrade) {
            return model;
        }
        let model = cx.new(|cx: &mut Context<Self>| {
            let list = Live::follow(TOPIC, cx, |event| match event {
                Event::Notifications(list) => Some(list),
                Event::NotificationPopup(_) => None,
            });
            cx.observe(&list, |_, _, cx| cx.notify()).detach();
            let this = cx.weak_entity();
            let popups = Daemon::on_event(TOPIC, cx, move |event, cx| {
                if let Event::NotificationPopup(notification) = event {
                    let _ = this.update(cx, |_, cx| cx.emit(Popped(notification)));
                }
            });
            Self { list, _popups: popups }
        });
        cx.global_mut::<Shared>().0 = Some(model.downgrade());
        model
    }

    pub fn list<'a>(&self, cx: &'a App) -> Option<&'a NotificationList> {
        self.list.read(cx).get()
    }

    pub fn items<'a>(&self, cx: &'a App) -> &'a [Notification] {
        self.list(cx).map_or(&[], |list| &list.items)
    }

    pub fn do_not_disturb(&self, cx: &App) -> bool {
        self.list(cx).is_some_and(|list| list.do_not_disturb)
    }

    /// Every notification a pop-up can be showing: the listed ones and the
    /// transient ones still open.
    pub fn popups(&self, cx: &App) -> Vec<Notification> {
        self.list(cx).map_or_else(Vec::new, |list| list.items.iter().chain(&list.transient).cloned().collect())
    }
}

/// `notification` was clicked on the output `output`: novad tells its app
/// and focuses the window it shows, or opens the app. Either way it stays
/// on the list, read.
pub fn open(notification: &Notification, output: &str, cx: &App) {
    let output = (!output.is_empty()).then(|| output.to_owned());
    Daemon::send(Command::OpenNotification { id: notification.id, output }, cx);
}

/// Its button `action` was clicked.
pub fn act(id: u32, action: &str, cx: &App) {
    Daemon::send(Command::NotificationAction { id, action: action.to_owned() }, cx);
}

pub fn dismiss(id: u32, cx: &App) {
    Daemon::send(Command::DismissNotification { id }, cx);
}

/// Every notification, or only `app`'s.
pub fn clear(app: Option<&str>, cx: &App) {
    Daemon::send(Command::ClearNotifications { app: app.map(str::to_owned) }, cx);
}

pub fn read_all(cx: &App) {
    Daemon::send(Command::ReadNotifications, cx);
}

pub fn set_do_not_disturb(on: bool, cx: &App) {
    Daemon::send(Command::SetDoNotDisturb { on }, cx);
}

pub fn mute(app: &str, muted: bool, cx: &App) {
    Daemon::send(Command::MuteNotificationApp { app: app.to_owned(), muted }, cx);
}
