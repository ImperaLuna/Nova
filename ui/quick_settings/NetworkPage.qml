pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import "../notifications"
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "network.js" as Network
import "quick_settings.js" as QuickSettings

// The network page: traffic over the last minute on the link in use, which
// link to prefer while a cable and Wi-Fi are both connected, the Wired row,
// Wi-Fi's switch and the networks in reach. A secured network that isn't
// saved asks for its password in a field under its row; a saved one, or
// the one it's on, has ⋯ with Disconnect and Forget. Properties in, signals
// out, like the other pages.
Item {
    id: page

    // A network event's data, or null before the first.
    property var network: null
    // A network_traffic event's data, or null before the first.
    property var traffic: null
    // Room the top row leaves on its right (the pill's buttons).
    property real headerInset: 0

    signal back
    signal wifiToggled(bool enabled)
    // `password` is null for a saved or open network.
    signal connectRequested(string ssid, var password)
    signal disconnectRequested
    signal forgetRequested(string ssid)
    signal preferPicked(string kind)

    readonly property var wifi: network?.wifi ?? null
    readonly property var wired: Network.wiredLink(network)
    readonly property string connecting: Network.connecting(network)
    readonly property var current: Network.current(network)
    readonly property var others: Network.others(network)

    // The network whose password field is open, what it holds, and whether
    // novad turned that password down (cleared on typing).
    property string asked: ""
    property string password: ""
    property bool wrong: false
    property bool showPassword: false
    // The network whose ⋯ menu is open, and the bottom right of its ⋯ in
    // the page.
    property string menuFor: ""
    property point menuAt: Qt.point(0, 0)
    readonly property var menuNet: [current].concat(others).find(net => net !== null && net.ssid === menuFor) ?? null

    // A wrong password (or none, for a saved network whose password
    // changed) opens the field again, the text kept to fix a typo; joining
    // clears it. Each attempt's outcome acts once: every network event
    // repeats it, and one from before the page opened stays shut.
    readonly property var attempt: wifi?.attempt ?? null
    readonly property string outcome: attempt ? `${attempt.ssid}\n${attempt.outcome}` : ""
    property string handled: ""
    property bool opened: false
    Component.onCompleted: {
        handled = outcome;
        opened = true;
    }
    onOutcomeChanged: {
        if (!opened || outcome === handled)
            return;
        handled = outcome;
        if (attempt?.outcome === "wrong_password") {
            asked = attempt.ssid;
            wrong = true;
        } else if (attempt?.outcome === "connected") {
            password = "";
        }
    }

    // A click on a network's row: join it, or ask for its password first.
    function pick(net: var) {
        menuFor = "";
        if (!Network.joinable(net) || net.connected || connecting !== "")
            return;
        if (!Network.needsPassword(net)) {
            connectRequested(net.ssid, null);
            return;
        }
        if (asked === net.ssid) {
            cancel();
            return;
        }
        asked = net.ssid;
        password = "";
        wrong = false;
    }

    function submit() {
        if (!Network.passwordFits(password))
            return;
        const ssid = asked;
        asked = "";
        wrong = false;
        connectRequested(ssid, password);
    }

    function cancel() {
        asked = "";
        password = "";
        wrong = false;
    }

    Column {
        id: rowsAbove
        width: parent.width
        spacing: Tokens.quickSettingsGap

        PageHeader {
            width: parent.width - page.headerInset
            title: qsTr("Network")
            onBack: page.back()
        }

        // Traffic on the link in use, the last minute and today.
        Rectangle {
            objectName: "traffic"
            width: parent.width
            height: card.implicitHeight + 2 * Tokens.pad
            radius: Tokens.radiusCard
            color: Tokens.surfaceRaised

            Column {
                id: card
                x: Tokens.pad
                y: Tokens.pad
                width: parent.width - 2 * Tokens.pad
                spacing: Tokens.gap

                readonly property bool online: (page.network?.primary ?? "") !== ""
                readonly property string icon: page.network?.primary === "wifi" ? Icons.WIFI : card.online ? Icons.WIRED : page.wifi ? Icons.WIFI_OFF : Icons.WIRED_OFF
                readonly property int mbps: Network.linkMbps(page.network)

                Row {
                    spacing: Tokens.gap

                    LineIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        path: card.icon
                        fill: Network.iconFill(card.icon)
                        size: Tokens.iconSizeSmall
                    }
                    Text {
                        objectName: "linkName"
                        anchors.verticalCenter: parent.verticalCenter
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeSmall
                        font.weight: Font.DemiBold
                        color: Tokens.fg
                        textFormat: Text.PlainText
                        text: Network.linkName(page.network)
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        visible: card.mbps > 0
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeTiny
                        color: Tokens.fgDim
                        text: qsTr("%1 link").arg(Network.linkSpeed(card.mbps))
                    }
                }

                Row {
                    visible: card.online
                    spacing: Tokens.pad

                    Speed {
                        objectName: "down"
                        color: Tokens.primary
                        arrow: "↓"
                        values: page.traffic?.down ?? []
                    }
                    Speed {
                        objectName: "up"
                        color: Tokens.secondary
                        arrow: "↑"
                        values: page.traffic?.up ?? []
                    }
                }

                TrafficGraph {
                    width: parent.width
                    traffic: card.online ? page.traffic : null
                }

                Item {
                    width: parent.width
                    height: past.implicitHeight

                    Text {
                        id: past
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSizeMicro
                        color: Tokens.fgDim
                        text: qsTr("last %1 s").arg(Protocol.NETWORK_TRAFFIC_HISTORY_SECS)
                    }
                    Text {
                        objectName: "today"
                        anchors.right: parent.right
                        visible: page.traffic !== null
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSizeMicro
                        color: Tokens.fgDim
                        text: page.traffic ? qsTr("today ↓ %1 ↑ %2").arg(QuickSettings.bytes(page.traffic.today_down)).arg(QuickSettings.bytes(page.traffic.today_up)) : ""
                    }
                }
            }
        }

        // Which link carries traffic while both are connected.
        Column {
            visible: Network.preferShown(page.network)
            width: parent.width
            spacing: Tokens.gap

            CardTitle {
                x: Tokens.labelNudge * 2
                text: qsTr("Prefer")
            }
            Segments {
                objectName: "prefer"
                width: parent.width
                accent: true
                options: [
                    {
                        key: "wired",
                        label: qsTr("Wired"),
                        icon: Icons.WIRED
                    },
                    {
                        key: "wifi",
                        label: qsTr("Wi-Fi"),
                        icon: Icons.WIFI,
                        fill: Icons.WIFI_DOT
                    }
                ]
                current: page.network?.prefer ?? ""
                onPicked: key => page.preferPicked(key)
            }
        }

        // The Ethernet port, while a cable is in.
        LinkRow {
            id: wiredRow
            objectName: "wired"
            visible: page.wired !== null
            width: parent.width
            selected: page.wired?.state === "connected"
            title: qsTr("Wired")
            subtitle: page.wired ? [page.wired.interface].concat(page.wired.speed_mbps ? [Network.linkSpeed(page.wired.speed_mbps)] : []).join(" · ") : ""
            role: page.wired?.state === "connected" ? Network.role(page.network, "wired") : ""

            LineIcon {
                parent: wiredRow.lead
                anchors.centerIn: parent
                path: page.wired?.state === "connected" ? Icons.WIRED : Icons.WIRED_OFF
                size: Tokens.iconSizeMenu
            }
        }

        // Wi-Fi's switch; a hardware switch or airplane key greys it out.
        LinkRow {
            id: wifiRow
            objectName: "wifiSwitch"
            visible: page.wifi !== null
            width: parent.width
            title: qsTr("Wi-Fi")
            onClicked: {
                if (!page.wifi.blocked)
                    page.wifiToggled(!page.wifi.enabled);
            }

            LineIcon {
                parent: wifiRow.lead
                anchors.centerIn: parent
                path: Network.wifiOn(page.network) ? Icons.WIFI : Icons.WIFI_OFF
                fill: Icons.WIFI_DOT
                size: Tokens.iconSizeMenu
            }
            ToggleSwitch {
                parent: wifiRow.end
                anchors.verticalCenter: parent.verticalCenter
                on: Network.wifiOn(page.network)
                opacity: page.wifi?.blocked ? Tokens.dimOpacity : 1
            }
        }
    }

    // The networks in reach, scrolling below the rows above.
    Flickable {
        id: list
        objectName: "networks"
        visible: Network.wifiOn(page.network)
        anchors.top: rowsAbove.bottom
        anchors.topMargin: Tokens.gap
        anchors.bottom: parent.bottom
        width: parent.width
        contentHeight: rows.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        // The menu stays where it opened: scrolling closes it.
        onContentYChanged: page.menuFor = ""

        Column {
            id: rows
            width: list.width

            NetworkEntry {
                visible: page.current !== null
                net: page.current
            }

            CardTitle {
                x: Tokens.pad
                visible: page.others.length > 0
                topPadding: Tokens.gap
                bottomPadding: Tokens.labelNudge * 2
                text: page.current ? qsTr("Other networks") : qsTr("Networks")
            }

            // By count, so a new event doesn't rebuild the rows (and drop the
            // password being typed, or a hover).
            Repeater {
                model: page.others.length

                NetworkEntry {
                    required property int index
                    net: page.others[index] ?? null
                }
            }

            Item {
                objectName: "scanning"
                visible: page.wifi?.scanning ?? false
                width: parent.width
                height: Tokens.powerRowHeight

                Spinner {
                    id: scanSpinner
                    anchors.verticalCenter: parent.verticalCenter
                    x: Tokens.pad
                    size: Tokens.iconSizeMenu
                    color: Tokens.fgDim
                    spinning: parent.visible
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    x: scanSpinner.x + scanSpinner.width + Tokens.pad
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSize
                    color: Tokens.fgDim
                    text: qsTr("Scanning…")
                }
            }
        }
    }

    // Disconnect (the one it's on) and Forget (a saved one), under the ⋯
    // that opened it, over everything: a click beside it closes it.
    MouseArea {
        anchors.fill: parent
        visible: page.menuNet !== null
        onClicked: page.menuFor = ""
    }
    Rectangle {
        objectName: "menu"
        visible: page.menuNet !== null
        x: Math.max(0, page.menuAt.x - width)
        y: page.menuAt.y + Tokens.labelNudge
        width: Tokens.networkMenuWidth
        height: menu.implicitHeight + 2 * Tokens.segmentPad
        radius: Tokens.radiusControl
        color: Tokens.bg
        border.width: Tokens.dividerWidth
        border.color: Tokens.glassRim

        Column {
            id: menu
            x: Tokens.segmentPad
            y: Tokens.segmentPad
            width: parent.width - 2 * x

            MenuItem {
                objectName: "disconnect"
                visible: page.menuNet?.connected ?? false
                icon: Icons.CLOSE
                label: qsTr("Disconnect")
                onClicked: {
                    page.menuFor = "";
                    page.disconnectRequested();
                }
            }
            MenuItem {
                objectName: "forget"
                visible: page.menuNet?.saved ?? false
                icon: Icons.TRASH
                label: qsTr("Forget")
                onClicked: {
                    const ssid = page.menuFor;
                    page.menuFor = "";
                    page.forgetRequested(ssid);
                }
            }
        }
    }

    // A network's row and its password field.
    component NetworkEntry: Column {
        id: entry

        property var net: null
        readonly property string ssid: net?.ssid ?? ""
        readonly property bool joinable: net ? Network.joinable(net) : false
        readonly property bool joining: ssid !== "" && page.connecting === ssid
        readonly property bool hasMenu: net !== null && (net.saved || net.connected) && !joining
        readonly property bool menuOpen: hasMenu && page.menuFor === ssid
        readonly property bool asking: ssid !== "" && page.asked === ssid

        width: rows.width

        LinkRow {
            id: row
            objectName: `network-${entry.ssid}`
            width: parent.width
            selected: entry.net?.connected ?? false
            // Not joinable still has its ⋯ (pick() won't join it).
            clickable: entry.joinable
            opacity: entry.joinable ? 1 : Tokens.dimOpacity
            title: entry.ssid
            subtitle: entry.net ? Network.networkLine(page.network, entry.net) : ""
            subtitleColor: page.attempt?.ssid === entry.ssid && page.attempt?.outcome === "failed" ? Tokens.error : Tokens.fgDim
            role: entry.net?.connected ? Network.role(page.network, "wifi") : ""
            onClicked: page.pick(entry.net)

            WifiIcon {
                parent: row.lead
                anchors.centerIn: parent
                strength: entry.net?.strength ?? 0
            }
            Spinner {
                parent: row.end
                anchors.verticalCenter: parent.verticalCenter
                visible: entry.joining
                spinning: visible
                size: Tokens.iconSizeSmall
            }
            LineIcon {
                parent: row.end
                anchors.verticalCenter: parent.verticalCenter
                visible: !entry.joining && entry.net !== null && Network.needsPassword(entry.net)
                path: Icons.LOCK
                size: Tokens.iconSizeSmall
                color: Tokens.fgDim
            }
            RoundButton {
                id: more
                objectName: "more"
                parent: row.end
                anchors.verticalCenter: parent.verticalCenter
                visible: entry.hasMenu && (row.hovered || entry.menuOpen || (entry.net?.connected ?? false))
                quiet: true
                size: Tokens.notificationCloseSize
                icon: Icons.DOTS
                fill: Icons.DOTS
                onClicked: {
                    page.menuAt = more.mapToItem(page, more.width, more.height);
                    page.menuFor = entry.menuOpen ? "" : entry.ssid;
                }
            }
        }

        // The password, Enter or Connect joins, Esc or Cancel closes it.
        Column {
            visible: entry.asking
            width: parent.width
            topPadding: Tokens.labelNudge * 2
            bottomPadding: Tokens.gap
            spacing: Tokens.gap

            Rectangle {
                x: Tokens.pad
                width: parent.width - 2 * Tokens.pad
                height: Tokens.searchHeight
                radius: height / 2
                color: Tokens.track
                border.width: Tokens.dividerWidth
                border.color: page.wrong ? Tokens.error : field.activeFocus ? Tokens.primary : Tokens.outline

                LineIcon {
                    id: lock
                    x: Tokens.pad
                    anchors.verticalCenter: parent.verticalCenter
                    path: Icons.LOCK
                    size: Tokens.iconSizeSmall
                    color: Tokens.fgDim
                }
                TextInput {
                    id: field
                    objectName: "password"
                    anchors.verticalCenter: parent.verticalCenter
                    x: lock.x + lock.width + Tokens.gap
                    width: eye.x - x - Tokens.gap
                    clip: true
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSmall
                    color: Tokens.fg
                    selectionColor: Tokens.secondary
                    selectedTextColor: Tokens.onAccent
                    echoMode: page.showPassword ? TextInput.Normal : TextInput.Password
                    passwordCharacter: "•"
                    text: page.password
                    onTextEdited: {
                        page.password = text;
                        page.wrong = false;
                    }
                    onVisibleChanged: {
                        if (visible)
                            forceActiveFocus();
                    }
                    Keys.onReturnPressed: page.submit()
                    Keys.onEnterPressed: page.submit()
                    Keys.onEscapePressed: page.cancel()

                    Text {
                        visible: field.text === ""
                        width: parent.width
                        elide: Text.ElideRight
                        font: field.font
                        color: Tokens.fgDim
                        textFormat: Text.PlainText
                        text: qsTr("Password for %1").arg(entry.ssid)
                    }
                }
                RoundButton {
                    id: eye
                    objectName: "eye"
                    anchors.right: parent.right
                    anchors.rightMargin: Tokens.labelNudge * 2
                    anchors.verticalCenter: parent.verticalCenter
                    quiet: true
                    size: Tokens.notificationCloseSize
                    icon: page.showPassword ? Icons.EYE_OFF : Icons.EYE
                    onClicked: page.showPassword = !page.showPassword
                }
            }

            Row {
                anchors.right: parent.right
                anchors.rightMargin: Tokens.pad
                spacing: Tokens.iconGap

                Text {
                    objectName: "wrong"
                    anchors.verticalCenter: parent.verticalCenter
                    visible: page.wrong
                    rightPadding: Tokens.gap
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeTiny
                    color: Tokens.error
                    text: qsTr("Wrong password")
                }
                PageButton {
                    text: qsTr("Cancel")
                    onClicked: page.cancel()
                }
                PageButton {
                    objectName: "connect"
                    text: qsTr("Connect")
                    accent: true
                    enabled: Network.passwordFits(page.password)
                    onClicked: page.submit()
                }
            }
        }
    }

    // A row: an icon in `lead`, a name with a line under it, a role chip
    // (Network.role's key; "In use" in the accent) and whatever goes in
    // `end` at its right.
    component LinkRow: Item {
        id: line

        required property string title
        property string subtitle: ""
        property color subtitleColor: Tokens.fgDim
        property string role: ""
        property bool selected: false
        // Clicks and hovers on the row itself (not what's in `end`).
        property bool clickable: true
        readonly property bool hovered: press.containsMouse
        readonly property Item lead: leadSlot
        readonly property Item end: endSlot
        signal clicked

        implicitHeight: Tokens.powerRowHeight

        Rectangle {
            anchors.fill: parent
            radius: Tokens.radiusRow
            color: press.containsMouse && line.clickable ? Tokens.hover : line.selected ? Tokens.surfaceRaised : "transparent"
        }
        MouseArea {
            id: press
            anchors.fill: parent
            hoverEnabled: true
            onClicked: line.clicked()
        }
        Item {
            id: leadSlot
            x: Tokens.pad
            width: Tokens.iconSizeMenu
            height: parent.height
        }
        Column {
            anchors.verticalCenter: parent.verticalCenter
            x: leadSlot.x + leadSlot.width + Tokens.pad
            width: endRow.x - x - Tokens.gap

            Text {
                width: parent.width
                elide: Text.ElideRight
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: Font.Medium
                color: Tokens.fg
                textFormat: Text.PlainText
                text: line.title
            }
            Text {
                visible: text !== ""
                width: parent.width
                elide: Text.ElideRight
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeTiny
                color: line.subtitleColor
                textFormat: Text.PlainText
                text: line.subtitle
            }
        }
        Row {
            id: endRow
            anchors.right: parent.right
            anchors.rightMargin: Tokens.pad
            anchors.verticalCenter: parent.verticalCenter
            height: parent.height
            spacing: Tokens.gap

            Rectangle {
                objectName: "role"
                anchors.verticalCenter: parent.verticalCenter
                visible: line.role !== ""
                readonly property bool inUse: line.role !== "backup"
                width: roleText.implicitWidth + 2 * Tokens.chipPad
                height: Tokens.chipHeight
                radius: height / 2
                color: inUse ? Tokens.primary : Tokens.track

                Text {
                    id: roleText
                    anchors.centerIn: parent
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeTiny
                    font.weight: parent.inUse ? Font.DemiBold : Font.Normal
                    color: parent.inUse ? Tokens.onAccent : Tokens.fgSoft
                    text: Network.roleLabel(line.role)
                }
            }
            Row {
                id: endSlot
                anchors.verticalCenter: parent.verticalCenter
                height: parent.height
                spacing: Tokens.gap
            }
        }
    }

    component MenuItem: Rectangle {
        id: item
        required property string icon
        required property string label
        signal clicked

        width: parent.width
        height: Tokens.segmentHeight
        radius: Tokens.radiusSegment
        color: itemPress.containsMouse ? Tokens.hover : "transparent"

        LineIcon {
            id: itemIcon
            x: Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            path: item.icon
            size: Tokens.iconSizeSmall
        }
        Text {
            x: itemIcon.x + itemIcon.width + Tokens.gap
            width: parent.width - x - Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            elide: Text.ElideRight
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeSmall
            color: Tokens.fg
            text: item.label
        }
        MouseArea {
            id: itemPress
            anchors.fill: parent
            hoverEnabled: true
            onClicked: item.clicked()
        }
    }

    component PageButton: Rectangle {
        id: button
        property alias text: label.text
        property bool accent: false
        signal clicked
        width: label.implicitWidth + 2 * Tokens.pad
        height: Tokens.confirmButtonHeight
        radius: Tokens.radiusSegment
        opacity: enabled ? 1 : Tokens.dimOpacity
        color: accent ? Tokens.primary : buttonPress.containsMouse ? Tokens.hover : Tokens.track

        Text {
            id: label
            anchors.centerIn: parent
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeTiny
            font.weight: button.accent ? Font.DemiBold : Font.Normal
            color: button.accent ? Tokens.onAccent : Tokens.fg
        }
        MouseArea {
            id: buttonPress
            anchors.fill: parent
            hoverEnabled: true
            onClicked: button.clicked()
        }
    }

    // A speed now: a dot in its line's color, the arrow and the number.
    component Speed: Row {
        id: speed
        required property color color
        required property string arrow
        required property var values
        spacing: Tokens.iconGap

        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.legendDot
            height: width
            radius: width / 2
            color: speed.color
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            font.family: Tokens.fontMono
            font.pixelSize: Tokens.fontSizeTraffic
            color: Tokens.fg
            text: `${speed.arrow} ${Network.speed(speed.values.length > 0 ? speed.values[speed.values.length - 1] : 0)}`
        }
    }

    // Down (primary) and up (secondary) on one scale, newest at the right,
    // over a quiet middle line.
    component TrafficGraph: Item {
        id: graph
        property var traffic: null
        readonly property real peak: Network.peak(traffic)
        implicitHeight: Tokens.trafficGraphHeight

        Rectangle {
            y: graph.height / 2
            width: graph.width
            height: Tokens.dividerWidth
            color: Tokens.outline
        }
        Shape {
            anchors.fill: parent
            visible: graph.traffic !== null
            preferredRendererType: Shape.CurveRenderer

            ShapePath {
                strokeColor: Tokens.primary
                strokeWidth: Tokens.graphLine
                fillColor: "transparent"
                joinStyle: ShapePath.RoundJoin
                PathSvg {
                    path: graph.traffic ? QuickSettings.graphPath(Network.scaled(graph.traffic.down, graph.peak), graph.width, graph.height, Protocol.NETWORK_TRAFFIC_HISTORY_SECS, false) : ""
                }
            }
            ShapePath {
                strokeColor: Tokens.secondary
                strokeWidth: Tokens.graphLine
                fillColor: "transparent"
                joinStyle: ShapePath.RoundJoin
                PathSvg {
                    path: graph.traffic ? QuickSettings.graphPath(Network.scaled(graph.traffic.up, graph.peak), graph.width, graph.height, Protocol.NETWORK_TRAFFIC_HISTORY_SECS, false) : ""
                }
            }
        }
    }
}
