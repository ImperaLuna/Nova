import QtQuick
import ".."
import "../../icons"
import "../../notifications/service"
import "../quick_settings.js" as QuickSettings

// notifications.list: the notifications in the room the first page leaves.
// While it shows, they're read here rather than popping up.
QsSlot {
    id: slot

    present: NotificationsService.ready
    readonly property bool listShown: qs.open && qs.page === QuickSettings.PAGE_MAIN
    // Its own copy: `qs` may be gone first when its screen goes.
    readonly property string outputName: qs.outputName
    onListShownChanged: NotificationsService.showList(outputName, listShown)
    Component.onCompleted: NotificationsService.showList(outputName, listShown)
    Component.onDestruction: NotificationsService.showList(outputName, false)

    NotificationList {
        anchors.fill: parent
        items: NotificationsService.items
        doNotDisturb: NotificationsService.doNotDisturb
        iconSource: name => AppIcons.source(name)
        now: slot.qs.now.getTime() / 1000
        // Opening one takes you to its app.
        onOpened: notification => {
            NotificationsService.open(notification, slot.qs.outputName);
            slot.qs.closeRequested();
        }
        onDismissed: id => NotificationsService.dismiss(id)
        onCleared: app => NotificationsService.clear(app)
        onMuted: app => NotificationsService.mute(app, true)
        onDoNotDisturbToggled: NotificationsService.setDoNotDisturb(!NotificationsService.doNotDisturb)
        onSettingsOpened: slot.qs.pageRequested(QuickSettings.PAGE_NOTIFICATIONS)
    }
}
