import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "network.js" as Network

// Wi-Fi's icon for a network of `strength` (0 to 100): the arcs its signal
// reaches drawn in `color`, the others faint, over the solid dot.
Item {
    id: icon

    property int strength: 100
    property color color: Tokens.fg
    property int size: Tokens.iconSizeMenu

    readonly property int arcs: Network.arcs(strength)
    readonly property var paths: [Icons.WIFI_ARC_INNER, Icons.WIFI_ARC_MIDDLE, Icons.WIFI_ARC_OUTER]

    implicitWidth: size
    implicitHeight: size

    LineIcon {
        path: icon.paths.slice(0, icon.arcs).join("")
        fill: Icons.WIFI_DOT
        size: icon.size
        color: icon.color
    }
    LineIcon {
        visible: icon.arcs < 3
        path: icon.paths.slice(icon.arcs).join("")
        size: icon.size
        color: Tokens.alpha(icon.color, Tokens.wifiArcFadedOpacity)
    }
}
