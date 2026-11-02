import QtQuick
import "../daemon/protocol.js" as Protocol
import "../notifications/service"

// Quick settings' button in the right-pill.buttons host (view
// `quick-settings.button`): opens and closes the panel on this screen, with
// the unread count and do not disturb from notifications when they're on.
QuickSettingsButton {
    // The screen's stage (Stage.qml).
    required property var stage

    open: stage.isOpen(Protocol.FEATURE_QUICK_SETTINGS)
    unread: NotificationsService.ready ? NotificationsService.unread : 0
    doNotDisturb: NotificationsService.ready && NotificationsService.doNotDisturb
    onClicked: stage.toggle(Protocol.FEATURE_QUICK_SETTINGS)
}
