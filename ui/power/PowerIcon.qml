import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol

// A power menu action's line icon, stroked in `color`.
LineIcon {
    required property string action

    size: Tokens.iconSizeMenu
    path: ({
            [Protocol.POWER_LOCK]: Icons.LOCK,
            [Protocol.POWER_SUSPEND]: Icons.MOON,
            [Protocol.POWER_LOG_OUT]: Icons.LOG_OUT,
            [Protocol.POWER_REBOOT]: Icons.REBOOT,
            [Protocol.POWER_POWER_OFF]: Icons.POWER
        })[action] ?? ""
}
