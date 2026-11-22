//! The notifications feature's messages: the notifications novad keeps as
//! the session's notification server, for the quick settings list and the
//! pop-ups, with do not disturb and muted apps.
//!
//! A client sends a [`Command`] (a click, a dismiss, do not disturb...);
//! novad publishes an [`Event`] on [`TOPIC`]. Other features post Nova's
//! own notifications (a device's low battery) through [`Post`], a port
//! novad's wiring fills with this feature's core. The work is
//! `nova-notifications` (`features/notifications/core`).

use nova_carrier::{Published, Topic};
use nova_peripherals_api::{Battery, PeripheralKind};
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for [`Event::Notifications`] and
/// [`Event::NotificationPopup`].
pub const TOPIC: Topic = Topic::named("notifications");

/// Sent by the UI. Arguments with unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// The notification `id` was clicked on `output` (feature
    /// `notifications`): the app hears of it and its window takes focus, or,
    /// when it can't hear of it, the app opens (as with `launch_app`). It
    /// stays on the list, read.
    OpenNotification {
        id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    /// The button `action` of notification `id` was clicked (feature
    /// `notifications`): the app hears of it, nothing takes focus, and it
    /// stays on the list, read.
    NotificationAction { id: u32, action: String },
    /// Closes the notification `id` and takes it off the list (feature
    /// `notifications`).
    DismissNotification { id: u32 },
    /// Takes every notification off the list, or only `app`'s (feature
    /// `notifications`).
    ClearNotifications {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app: Option<String>,
    },
    /// The list was seen: no notification is unread any more (feature
    /// `notifications`).
    ReadNotifications,
    /// Turns do not disturb on or off (feature `notifications`).
    /// Remembered across restarts.
    SetDoNotDisturb { on: bool },
    /// Mutes or unmutes `app`'s notifications (feature `notifications`).
    /// Remembered across restarts.
    MuteNotificationApp { app: String, muted: bool },
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The notifications kept for the quick settings list, do not disturb
    /// and the muted apps (feature `notifications`). Topic
    /// [`TOPIC`], sent right after subscribing and whenever
    /// any of it changes.
    Notifications(NotificationList),
    /// A notification to pop up: a new one, or one an app replaced while
    /// it may still show (feature `notifications`). Never for a muted app,
    /// nor during do not disturb unless it's critical. Topic
    /// [`TOPIC`].
    NotificationPopup(Notification),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// What the notifications list holds (feature `notifications`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationList {
    /// The newest first, at most [`MAX_NOTIFICATIONS`].
    pub items: Vec<Notification>,
    /// No pop-ups, except critical ones.
    pub do_not_disturb: bool,
    /// Apps, by [`Notification::app`], whose notifications are hidden: not
    /// in `items`, never popping up, never unread. Those from before the
    /// app was muted come back once it's unmuted; those it sends meanwhile
    /// aren't kept.
    pub muted: Vec<String>,
    /// Every app that sent one lately or is muted, the latest first, for
    /// the settings page's per-app switches.
    pub apps: Vec<NotificationApp>,
    /// Transient notifications still open: they pop up but aren't listed,
    /// and their pop-ups go once they leave this.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transient: Vec<Notification>,
}

/// How many notifications are kept; the oldest go first.
pub const MAX_NOTIFICATIONS: usize = 100;

/// An app on the notification settings page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationApp {
    /// As [`Notification::app`].
    pub name: String,
    /// As [`Notification::icon`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// As [`Notification::app_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
}

/// One notification an app sent (org.freedesktop.Notifications).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notification {
    /// novad's id for it, the one the app knows it by.
    pub id: u32,
    /// The app's name as it gave it, or its desktop entry's id.
    pub app: String,
    /// The app's icon: an absolute path to an image, the file it gave or
    /// the icon theme's for the name it gave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The app's desktop file id (`discord.desktop`), when it said which.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    /// One line, plain text.
    pub summary: String,
    /// Plain text: any markup is taken out.
    pub body: String,
    /// A picture that goes with it (a sender's photo), as an absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Its buttons, in the app's order. Clicking the notification itself
    /// is `has_default`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<NotificationAction>,
    /// The app does something when it's clicked (its default action).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_default: bool,
    pub urgency: Urgency,
    /// How far along something it reports is, 0 to 100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<u8>,
    /// How long it may show as a pop-up, as the app asked: `None` for the
    /// UI's default, 0 for until it's closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,
    /// When it came, or was last replaced, in seconds since the Unix epoch.
    pub time: i64,
    /// Not seen in the list yet (`read_notifications`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unread: bool,
    /// The app that sent it is still there to hear about clicks: false for
    /// those kept from before novad started.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub live: bool,
    /// The device a low battery alert of Nova's own is about, for its
    /// icon: never set on an app's notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<NotificationDevice>,
}

/// The device a low battery alert is about: what it is and its charge
/// when the alert was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationDevice {
    pub kind: PeripheralKind,
    pub battery: Battery,
}

/// One of a notification's buttons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationAction {
    /// What `notification_action` sends back.
    pub key: String,
    pub label: String,
}

/// How much a notification matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    /// Pops up even during do not disturb, and stays until it's closed.
    Critical,
}

/// A notification of Nova's own (a device's low battery), posted through
/// [`Post::post`].
#[derive(Debug, Clone, PartialEq)]
pub struct Own {
    /// Who it reads as from, on its app line.
    pub app: String,
    pub summary: String,
    pub body: String,
    pub critical: bool,
    /// The id of an earlier one it takes the place of, or 0.
    pub replaces: u32,
    pub device: Option<NotificationDevice>,
}

/// Where another feature posts Nova's own notifications: a port its core
/// holds, which novad's wiring fills with this feature's core while it runs.
pub trait Post: Send + Sync {
    /// Shows `own` as if an app sent it, and returns its id (0 once stopped).
    fn post(&self, own: Own) -> u32;

    /// The newest listed notification `matches` picks, by id: the one a
    /// notification of Nova's own takes the place of when it knows no
    /// earlier id, such as after a restart.
    fn listed(&self, matches: &dyn Fn(&Notification) -> bool) -> Option<u32>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_peripherals_api::BatteryLevel;

    fn notifications() -> NotificationList {
        NotificationList {
            items: vec![
                Notification {
                    id: 7,
                    app: "Firefox".into(),
                    icon: Some("firefox".into()),
                    app_id: Some("org.mozilla.firefox.desktop".into()),
                    summary: "Download complete".into(),
                    body: "niri-26.04.tar.gz".into(),
                    image: None,
                    actions: vec![NotificationAction { key: "open".into(), label: "Open file".into() }],
                    has_default: true,
                    urgency: Urgency::Normal,
                    progress: None,
                    timeout_ms: None,
                    time: 1_761_400_000,
                    unread: true,
                    live: true,
                    device: None,
                },
                Notification {
                    id: 3,
                    app: "Discord".into(),
                    icon: None,
                    app_id: None,
                    summary: "Mara".into(),
                    body: "sent you the map".into(),
                    image: Some("/home/mara/.local/state/nova/notifications/3.png".into()),
                    actions: vec![],
                    has_default: false,
                    urgency: Urgency::Critical,
                    progress: Some(40),
                    timeout_ms: Some(0),
                    time: 1_761_399_000,
                    unread: false,
                    live: false,
                    device: Some(NotificationDevice {
                        kind: PeripheralKind::Mouse,
                        battery: Battery::Level(BatteryLevel::Critical),
                    }),
                },
            ],
            do_not_disturb: false,
            muted: vec!["Discord".into()],
            apps: vec![
                NotificationApp {
                    name: "Firefox".into(),
                    icon: Some("firefox".into()),
                    app_id: Some("org.mozilla.firefox.desktop".into()),
                },
                NotificationApp { name: "Discord".into(), icon: None, app_id: None },
            ],
            transient: vec![],
        }
    }

    #[test]
    fn notifications_reject_unknown_fields() {
        let list = serde_json::to_value(notifications()).unwrap();
        assert!(serde_json::from_value::<NotificationList>(list.clone()).is_ok());
        for pointer in ["", "/items/0", "/items/0/actions/0", "/apps/0"] {
            let mut extra = list.clone();
            extra.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("extra".into(), 1.into());
            assert!(serde_json::from_value::<NotificationList>(extra).is_err(), "{pointer}");
        }
    }

    #[test]
    fn its_events_are_on_its_topic() {
        assert_eq!(Event::Notifications(notifications()).topic(), Some(TOPIC));
        assert_eq!(Event::NotificationPopup(notifications().items[0].clone()).topic(), Some(TOPIC));
    }
}
