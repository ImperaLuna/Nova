import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// A check box of Nova Files' dialogs: an outlined square, filled in the
// accent with a tick while `checked`, faded while disabled. It only
// shows: what holds it takes the tap, so a label beside it can too.
Rectangle {
    id: box

    property bool checked: false

    width: Tokens.menuCheckSize
    height: Tokens.menuCheckSize
    radius: Tokens.menuCheckRadius
    opacity: enabled ? 1 : Tokens.filesCheckDisabledOpacity
    color: checked ? Tokens.primary : "transparent"
    border.width: checked ? 0 : Tokens.strokeThin
    border.color: Tokens.filesFaint

    LineIcon {
        anchors.centerIn: parent
        visible: box.checked
        path: Icons.CHECK
        size: Tokens.menuCheckIconSize
        color: Tokens.onAccent
    }
}
