pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../launcher/service"
import "../../daemon/protocol.js" as Protocol
import "../notifications.js" as Notifications

// Notifications' link to novad: the list, do not disturb and the muted
// apps for quick settings, the unread count for the badge, and the
// pop-ups' arrivals. The pop-ups (../NotificationPopups.qml) and quick
// settings read and act through here.
Singleton {
    id: root

    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_NOTIFICATIONS) && Daemon.notifications !== null
    readonly property var current: Daemon.notifications
    readonly property var items: current?.items ?? []
    // Transient ones still open: not listed, but their pop-ups stay while
    // they're here.
    readonly property var transients: current?.transient ?? []
    // Every notification a pop-up can be showing.
    readonly property var popUps: items.concat(transients)
    readonly property bool doNotDisturb: current?.do_not_disturb ?? false
    readonly property var muted: current?.muted ?? []
    readonly property var apps: current?.apps ?? []
    readonly property int unread: Notifications.unread(items)

    // Screens where quick settings is open, which shows the list: no
    // pop-ups meanwhile, and what's in it is read once it closes.
    property list<string> listShown: []
    readonly property bool listOpen: listShown.length > 0

    // A notification to pop up.
    signal popped(var notification)

    Connections {
        target: Daemon
        function onNotificationPopped(notification: var) {
            root.popped(notification);
        }
    }

    // Quick settings started (`open`) or stopped showing the list on the
    // screen `name`. What it showed is read once it's gone.
    function showList(name: string, open: bool) {
        const shown = listShown.includes(name);
        if (open === shown)
            return;
        const others = listShown.filter(screen => screen !== name);
        listShown = open ? [...others, name] : others;
        if (!open && unread > 0)
            Daemon.call(Protocol.CMD_READ_NOTIFICATIONS);
    }

    // `notification` was clicked: its app takes the click, or opens on
    // `output`. Either way it leaves the list.
    function open(notification: var, output: string) {
        const does = Notifications.clickDoes(notification);
        if (does === "action") {
            act(notification.id, null);
            return;
        }
        if (does === "launch" && LauncherService.ready)
            LauncherService.launch(notification.app_id, output, false, error => {
                if (error)
                    console.warn(`nova: can't open ${notification.app_id}: ${error}`);
            });
        dismiss(notification.id);
    }

    // One of its buttons (`action` its key), or the click itself (null).
    function act(id: int, action: var) {
        Daemon.call(Protocol.CMD_NOTIFICATION_ACTION, action === null ? {
            id: id
        } : {
            id: id,
            action: action
        });
    }

    function dismiss(id: int) {
        Daemon.call(Protocol.CMD_DISMISS_NOTIFICATION, {
            id: id
        });
    }

    // Every notification, or only `app`'s.
    function clear(app: var) {
        Daemon.call(Protocol.CMD_CLEAR_NOTIFICATIONS, app === null ? {} : {
            app: app
        });
    }

    function setDoNotDisturb(on: bool) {
        Daemon.call(Protocol.CMD_SET_DO_NOT_DISTURB, {
            on: on
        });
    }

    function mute(app: string, muted: bool) {
        Daemon.call(Protocol.CMD_MUTE_NOTIFICATION_APP, {
            app: app,
            muted: muted
        });
    }

    // What an Image shows for an app icon (a theme name or a path).
    function iconSource(icon: string): string {
        if (!icon)
            return "";
        if (icon.startsWith("/"))
            return Notifications.fileUrl(icon);
        return Quickshell.iconPath(icon, true);
    }
}
