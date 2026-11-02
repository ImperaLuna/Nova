import QtQuick
import "../bar"
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol

// The orange power button in the right-pill.buttons host (view
// `power.button`): opens and closes the power menu on this screen, which
// closes quick settings when it's open.
PillButton {
    // The screen's stage (Stage.qml).
    required property var stage

    icon: Icons.POWER
    ink: Tokens.primary
    onClicked: stage.toggle(Protocol.FEATURE_POWER)
}
