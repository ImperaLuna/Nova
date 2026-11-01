pragma ComponentBehavior: Bound

import QtQuick
import "../notifications"
import "../theme"
import "../theme/icons.js" as Icons
import "bluetooth.js" as Bluetooth
import "devices.js" as Devices

// The Bluetooth page: the switch, then the devices connected, the known
// ones and the ones nearby. A click on a device does what it needs: a new
// one is paired, trusted and connected, a known one connects, a connected
// one disconnects. A right click opens its menu: Disconnect and Forget for
// a connected one, Forget for a known one. Pairing questions and failures show
// under the device's row. `bluetooth` is a bluetooth event's data (null: none
// yet); properties in, signals out, like the other pages.
Item {
    id: page

    property var bluetooth: null
    // Room the top row leaves on its right (the pill's buttons).
    property real headerInset: 0

    signal back
    signal powerToggled(bool on)
    signal connectRequested(string address)
    signal disconnectRequested(string address)
    signal forgetRequested(string address)
    // A pairing question's id and its answer ("accept", "reject" or {code}).
    signal answered(int id, var answer)

    readonly property string radio: bluetooth?.radio ?? "missing"
    readonly property bool on: radio === "on"
    readonly property var devices: bluetooth?.devices ?? []
    readonly property var pairing: bluetooth?.pairing ?? null
    readonly property var connected: Bluetooth.inSection(devices, "connected")
    readonly property var mine: Bluetooth.inSection(devices, "mine")
    readonly property var nearby: Bluetooth.inSection(devices, "nearby")
    // A question about a device the lists don't show goes above them.
    readonly property bool unlistedPairing: pairing !== null && !devices.some(device => device.address === pairing.address)

    // The connected device whose Disconnect and Forget show, and the known
    // one asking to confirm Forget (addresses, or "").
    property string opened: ""
    property string forgetting: ""
    // Failures closed with Close, by address: hidden until the next one.
    property var closed: ({})
    // What's typed into a PIN field, kept while the rows below a new device
    // shift.
    property string draft: ""

    onPairingChanged: {
        if (!pairing)
            draft = "";
    }
    function act(device: var) {
        forgetting = "";
        switch (Bluetooth.click(device)) {
        case "connect":
            opened = "";
            connectRequested(device.address);
            break;
        case "disconnect":
            opened = "";
            disconnectRequested(device.address);
            break;
        }
    }

    // A right click: the device's menu, or shut it when it's open.
    function openMenu(device: var) {
        switch (Bluetooth.menu(device)) {
        case "actions":
            forgetting = "";
            opened = opened === device.address ? "" : device.address;
            break;
        case "forget":
            opened = "";
            forgetting = forgetting === device.address ? "" : device.address;
            break;
        }
    }

    Column {
        id: head
        width: page.width
        spacing: Tokens.quickSettingsGap

        Item {
            width: parent.width - page.headerInset
            height: header.implicitHeight

            PageHeader {
                id: header
                width: parent.width
                title: qsTr("Bluetooth")
                onBack: page.back()
            }

            ToggleSwitch {
                objectName: "power"
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                on: page.on
                opacity: page.radio === "switched_off" || page.radio === "missing" ? Tokens.dimOpacity : 1

                MouseArea {
                    anchors.fill: parent
                    enabled: page.radio === "on" || page.radio === "off"
                    onClicked: page.powerToggled(!page.on)
                }
            }
        }

        // Why it can't turn on, when it can't. Plain off is the switch's.
        Message {
            visible: page.radio === "missing" || page.radio === "switched_off"
            width: parent.width
            text: page.radio === "missing" ? qsTr("No Bluetooth adapter") : qsTr("Blocked by a hardware switch")
        }
    }

    // The lists, scrolling once they're longer than the page.
    Flickable {
        id: lists
        objectName: "lists"
        anchors.top: head.bottom
        anchors.topMargin: Tokens.gap
        anchors.bottom: parent.bottom
        width: parent.width
        contentHeight: sections.height
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: sections
            width: lists.width

            PairingPrompt {
                visible: page.on && page.unlistedPairing
                width: parent.width
                request: page.pairing ?? Bluetooth.NO_REQUEST
                name: request.address
                onAnswered: answer => page.answered(request.id, answer)
            }

            Section {
                title: qsTr("Connected")
                list: page.on ? page.connected : []
            }
            // Known devices show while it's off too, dimmed: they reconnect
            // once it's on.
            Section {
                title: qsTr("My devices")
                list: page.on || page.radio === "off" ? page.mine : []
            }
            Section {
                title: qsTr("Nearby")
                list: page.on ? page.nearby : []
                shown: page.on
                spinning: page.bluetooth?.scanning ?? false

                Text {
                    visible: page.nearby.length === 0 && !(page.bluetooth?.scanning ?? false)
                    leftPadding: Tokens.pad
                    topPadding: Tokens.labelNudge * 2
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSmall
                    color: Tokens.fgDim
                    text: qsTr("None found")
                }
            }
        }
    }

    // A titled list of devices. Rows by count, so a new event doesn't
    // rebuild them (and drop a hover or a PIN being typed).
    component Section: Column {
        id: section
        required property string title
        required property var list
        property bool shown: list.length > 0
        property bool spinning: false
        default property alias extra: after.data

        width: sections.width
        visible: shown
        topPadding: Tokens.gap

        Item {
            width: parent.width
            height: label.implicitHeight + Tokens.gap

            CardTitle {
                id: label
                x: Tokens.pad
                text: section.title
            }
            Spinner {
                anchors.right: parent.right
                anchors.rightMargin: Tokens.pad
                anchors.verticalCenter: label.verticalCenter
                size: Tokens.iconSizeSmall
                visible: section.spinning
                color: Tokens.fgDim
            }
        }

        Repeater {
            id: rows
            model: section.list.length

            DeviceEntry {
                required property int index
                device: section.list[index] ?? null
                width: section.width
            }
        }

        Column {
            id: after
            width: parent.width
        }
    }

    // A device's row, and what shows under it: its pairing question, its
    // failure, Disconnect and Forget, or Forget's confirm.
    component DeviceEntry: Column {
        id: entry
        required property var device
        readonly property string address: device?.address ?? ""
        readonly property bool asking: page.pairing !== null && page.pairing.address === address
        readonly property string busy: Bluetooth.busyText(device?.status ?? "", asking)
        readonly property bool failed: (device?.error ?? "") !== "" && page.closed[address] !== device.error
        spacing: Tokens.labelNudge * 2

        Item {
            objectName: `device-${entry.address}`
            width: parent.width
            height: Tokens.powerRowHeight
            opacity: page.on ? 1 : Tokens.dimOpacity

            Rectangle {
                anchors.fill: parent
                radius: Tokens.radiusRow
                color: press.containsMouse ? Tokens.hover : page.opened === entry.address || entry.asking ? Tokens.surfaceRaised : "transparent"
            }
            MouseArea {
                id: press
                anchors.fill: parent
                hoverEnabled: true
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                enabled: page.on && entry.device !== null && (Bluetooth.click(entry.device) !== "" || Bluetooth.menu(entry.device) !== "")
                onClicked: mouse => mouse.button === Qt.RightButton ? page.openMenu(entry.device) : page.act(entry.device)
            }

            LineIcon {
                id: glyph
                anchors.verticalCenter: parent.verticalCenter
                x: Tokens.pad
                path: Bluetooth.icon(entry.device?.kind ?? "")
                size: Tokens.iconSizeMenu
            }
            // Its name alone: the icon says what it is, the section where
            // it stands.
            Text {
                anchors.verticalCenter: parent.verticalCenter
                x: glyph.x + glyph.width + Tokens.pad
                width: end.x - x - Tokens.gap
                elide: Text.ElideRight
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: Font.Medium
                color: Tokens.fg
                textFormat: Text.PlainText
                text: entry.device?.name ?? ""
            }

            // Busy: what's happening and a spinner. Connected: its battery.
            // Otherwise what a click does, on hover, and ⋯ for a known one.
            Row {
                id: end
                anchors.right: parent.right
                anchors.rightMargin: Tokens.pad
                anchors.verticalCenter: parent.verticalCenter
                spacing: Tokens.gap
                visible: page.on

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    visible: text !== ""
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSmall
                    color: entry.busy !== "" ? Tokens.primary : Tokens.fgDim
                    text: entry.busy !== "" ? entry.busy : !press.containsMouse ? "" : entry.device?.status === "nearby" ? qsTr("Pair") : entry.device?.status === "paired" ? qsTr("Connect") : entry.device?.status === "connected" ? qsTr("Disconnect") : ""
                }
                Spinner {
                    anchors.verticalCenter: parent.verticalCenter
                    visible: entry.busy !== ""
                    size: Tokens.iconSizeSmall
                    color: Tokens.primary
                }
                // Stops pairing or connecting: novad cancels what's under way.
                RoundButton {
                    objectName: "stop"
                    anchors.verticalCenter: parent.verticalCenter
                    visible: !entry.asking && (entry.device?.status === "connecting" || entry.device?.status === "pairing")
                    icon: Icons.CLOSE
                    quiet: true
                    onClicked: page.disconnectRequested(entry.address)
                }
                Row {
                    anchors.verticalCenter: parent.verticalCenter
                    visible: entry.busy === "" && entry.device?.status === "connected" && entry.device?.battery !== undefined
                    spacing: Tokens.iconGap

                    DeviceBattery {
                        anchors.verticalCenter: parent.verticalCenter
                        fill: (entry.device?.battery ?? 0) / 100
                        low: Devices.low(entry.device?.battery ?? -1, "", false)
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSizeSmall
                        color: Tokens.fgDim
                        text: `${entry.device?.battery ?? 0}%`
                    }
                }
                RoundButton {
                    objectName: "more"
                    anchors.verticalCenter: parent.verticalCenter
                    visible: entry.busy === "" && entry.device?.status === "paired"
                    icon: Icons.DOTS
                    quiet: true
                    on: page.forgetting === entry.address
                    onClicked: {
                        page.opened = "";
                        page.forgetting = page.forgetting === entry.address ? "" : entry.address;
                    }
                }
            }
        }

        PairingPrompt {
            visible: entry.asking && page.on
            width: parent.width
            request: entry.asking ? page.pairing : Bluetooth.NO_REQUEST
            name: entry.device?.name ?? ""
            draft: page.draft
            onDrafted: text => page.draft = text
            onAnswered: answer => page.answered(request.id, answer)
        }

        Card {
            objectName: "failure"
            visible: entry.failed && !entry.asking && page.on
            tone: Tokens.error
            text: entry.device?.error ?? ""

            PromptButton {
                text: qsTr("Close")
                onClicked: {
                    const closed = Object.assign({}, page.closed);
                    closed[entry.address] = entry.device.error;
                    page.closed = closed;
                }
            }
            PromptButton {
                objectName: "retry"
                visible: entry.device?.status !== "connected"
                text: qsTr("Try again")
                accent: true
                onClicked: page.act(entry.device)
            }
        }

        Card {
            objectName: "connectedActions"
            visible: page.opened === entry.address && entry.device?.status === "connected" && page.on
            text: ""

            PromptButton {
                objectName: "disconnect"
                text: qsTr("Disconnect")
                onClicked: {
                    page.opened = "";
                    page.disconnectRequested(entry.address);
                }
            }
            PromptButton {
                objectName: "forget"
                text: qsTr("Forget")
                onClicked: {
                    page.opened = "";
                    page.forgetRequested(entry.address);
                }
            }
        }

        Card {
            objectName: "confirmForget"
            visible: page.forgetting === entry.address && page.on
            text: qsTr("Forget %1?").arg(entry.device?.name ?? "")

            PromptButton {
                text: qsTr("Cancel")
                onClicked: page.forgetting = ""
            }
            PromptButton {
                objectName: "forgetConfirm"
                text: qsTr("Forget")
                accent: true
                onClicked: {
                    page.forgetting = "";
                    page.forgetRequested(entry.address);
                }
            }
        }
    }

    // A card under a row: a line of text, then its buttons at the end.
    component Card: Rectangle {
        id: card
        required property string text
        property color tone: Tokens.fg
        default property alias buttons: buttonRow.data

        width: parent?.width ?? 0
        height: visible ? cardContent.implicitHeight + 2 * Tokens.pad : 0
        radius: Tokens.radiusRow
        color: Tokens.alpha(tone, Tokens.pairingTintOpacity)

        Column {
            id: cardContent
            x: Tokens.pad
            y: Tokens.pad
            width: parent.width - 2 * Tokens.pad
            spacing: Tokens.gap

            Text {
                visible: card.text !== ""
                width: parent.width
                wrapMode: Text.Wrap
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                color: Tokens.fg
                textFormat: Text.PlainText
                text: card.text
            }
            Row {
                id: buttonRow
                anchors.right: parent.right
                spacing: Tokens.iconGap
            }
        }
    }

    // Why it can't turn on.
    component Message: Rectangle {
        id: message
        required property string text

        height: visible ? messageText.implicitHeight + 2 * Tokens.pad : 0
        radius: Tokens.radiusCard
        color: Tokens.surfaceRaised

        Text {
            id: messageText
            x: Tokens.pad
            y: Tokens.pad
            width: parent.width - 2 * Tokens.pad
            wrapMode: Text.Wrap
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            font.weight: Font.DemiBold
            color: Tokens.fg
            text: message.text
        }
    }
}
