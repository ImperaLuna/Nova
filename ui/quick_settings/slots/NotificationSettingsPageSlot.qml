import QtQuick
import ".."
import "../../icons"
import "../../notifications/service"
import "../quick_settings.js" as QuickSettings

// notifications.page: do not disturb and muting apps.
QsSlot {
    id: slot

    NotificationSettingsPage {
        anchors.fill: parent
        headerInset: slot.qs.headerInset
        doNotDisturb: NotificationsService.doNotDisturb
        apps: NotificationsService.apps
        muted: NotificationsService.muted
        iconSource: name => AppIcons.source(name)
        onBack: slot.qs.pageRequested(QuickSettings.PAGE_MAIN)
        onDoNotDisturbToggled: NotificationsService.setDoNotDisturb(!NotificationsService.doNotDisturb)
        onMuteToggled: (app, muted) => NotificationsService.mute(app, muted)
    }
}
