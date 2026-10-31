pragma ComponentBehavior: Bound

import QtQuick
import "../notifications"
import "../theme"
import "../theme/icons.js" as Icons
import "bluetooth.js" as Bluetooth
import "devices.js" as Devices

// The Bluetooth page: the switch, then the devices connected, the known
// ones and the ones nearby. One click on a device does what it needs: a new
// one is paired, trusted and connected, a known one connects, a connected
// one shows Disconnect and Forget. Pairing questions and failures show
// under the device's row. `state` is a bluetooth event's data (null: none
// yet); properties in, signals out, like the other pages.
Item {
    id: page

    property var state: null
    // Room the top row leaves on its right (the pill's buttons).
    property real headerInset: 0

    signal back
    signal powerToggled(bool on)
    signal connectRequested(string address)
    signal disconnectRequested(string address)
    signal forgetRequested(string address)
    // A pairing question's id and its answer ("accept", "reject" or {code}).
    signal answered(int id, var answer)

    readonly property string radio: state?.radio ?? "missing"
    readonly property bool on: radio === "on"
    readonly property var devices: state?.devices ?? []
    readonly property var pairing: state?.pairing ?? null
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
    // Devices being paired now: once one connects, a note says it's done.
    property var pairingNow: []
    property string note: ""

    onPairingChanged: {
        if (!pairing)
            draft = "";
    }
    onDevicesChanged: {
        const done = [];
        const still = [];
        for (const address of pairingNow) {
            const device = devices.find(device => device.address === address);
            if (device?.status === "connected")
                done.push(device.name);
            else if (device && (device.status === "pairing" || device.status === "connecting"))
                still.push(address);
        }
        for (const device of devices) {
            if (device.status === "pairing" && !still.includes(device.address))
                still.push(device.address);
        }
        pairingNow = still;
        if (done.length > 0) {
            note = qsTr("%1 is paired and connected. It reconnects by itself from now on.").arg(done[done.length - 1]);
            noteTimer.restart();
        }
    }

    Timer {
        id: noteTimer
        interval: Tokens.durationPairedNote
        onTriggered: page.note = ""
    }

    function act(device: var) {
        forgetting = "";
        switch (Bluetooth.click(device)) {
        case "connect":
            opened = "";
            connectRequested(device.address);
            break;
        case "open":
            opened = opened === device.address ? "" : device.address;
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

        // Why it's off, when it is.
        Message {
            visible: !page.on
            width: parent.width
            title: page.radio === "missing" ? qsTr("No Bluetooth adapter") : page.radio === "switched_off" ? qsTr("Off by a hardware switch") : qsTr("Bluetooth is off")
            body: page.radio === "missing" ? qsTr("This computer has no Bluetooth, or its adapter isn't working.") : page.radio === "switched_off" ? qsTr("This is a hardware block, so Nova can't turn it on. Flip the wireless switch or key (often Fn plus a key with an antenna), or check the BIOS, and Bluetooth comes back here by itself.") : qsTr("Turn it on to use your devices. Known ones reconnect by themselves.")
        }

        // Pairing went through.
        Rectangle {
            objectName: "note"
            visible: page.on && page.note !== ""
            width: parent.width
            height: noteText.implicitHeight + 2 * Tokens.pad
            radius: Tokens.radiusRow
            color: Tokens.alpha(Tokens.primary, Tokens.pairingTintOpacity)

            LineIcon {
                id: noteIcon
                x: Tokens.pad
                y: Tokens.pad
                path: Icons.BLUETOOTH
                size: Tokens.iconSizeSmall
                color: Tokens.primary
            }
            Text {
                id: noteText
                x: noteIcon.x + noteIcon.width + Tokens.gap
                y: Tokens.pad
                width: parent.width - x - Tokens.pad
                wrapMode: Text.Wrap
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                color: Tokens.fg
                textFormat: Text.PlainText
                text: page.note
            }
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
                spinning: page.state?.scanning ?? false

                Text {
                    visible: page.nearby.length === 0
                    leftPadding: Tokens.pad
                    topPadding: Tokens.labelNudge * 2
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSmall
                    color: Tokens.fgDim
                    text: page.state?.scanning ? qsTr("Looking for devices… Put yours in pairing mode.") : qsTr("No new devices nearby.")
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
                enabled: page.on && entry.device !== null && Bluetooth.click(entry.device) !== ""
                onClicked: page.act(entry.device)
            }

            LineIcon {
                id: glyph
                anchors.verticalCenter: parent.verticalCenter
                x: Tokens.pad
                path: Bluetooth.icon(entry.device?.kind ?? "")
                size: Tokens.iconSizeMenu
            }
            Column {
                anchors.verticalCenter: parent.verticalCenter
                x: glyph.x + glyph.width + Tokens.pad
                width: end.x - x - Tokens.gap

                Text {
                    width: parent.width
                    elide: Text.ElideRight
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSize
                    font.weight: Font.Medium
                    color: Tokens.fg
                    textFormat: Text.PlainText
                    text: entry.device?.name ?? ""
                }
                Text {
                    width: parent.width
                    elide: Text.ElideRight
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeTiny
                    color: Tokens.fgDim
                    text: entry.device ? Bluetooth.rowSubtitle(entry.device) : ""
                }
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
                    font.pixelSize: Tokens.fontSizeTiny
                    color: entry.busy !== "" ? Tokens.primary : Tokens.fgDim
                    text: entry.busy !== "" ? entry.busy : !press.containsMouse ? "" : entry.device?.status === "nearby" ? qsTr("Pair") : entry.device?.status === "paired" ? qsTr("Connect") : ""
                }
                Spinner {
                    anchors.verticalCenter: parent.verticalCenter
                    visible: entry.busy !== ""
                    size: Tokens.iconSizeSmall
                    color: Tokens.primary
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
                        font.pixelSize: Tokens.fontSizeTiny
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
            text: qsTr("Forget %1? You'll need to pair it again to use it.").arg(entry.device?.name ?? "")

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

    // A title and a line: why it's off.
    component Message: Rectangle {
        id: message
        required property string title
        required property string body

        height: visible ? messageText.implicitHeight + 2 * Tokens.pad : 0
        radius: Tokens.radiusCard
        color: Tokens.surfaceRaised

        Column {
            id: messageText
            x: Tokens.pad
            y: Tokens.pad
            width: parent.width - 2 * Tokens.pad
            spacing: Tokens.labelNudge * 2

            Text {
                width: parent.width
                wrapMode: Text.Wrap
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: Font.DemiBold
                color: Tokens.fg
                text: message.title
            }
            Text {
                width: parent.width
                wrapMode: Text.Wrap
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                color: Tokens.fgDim
                text: message.body
            }
        }
    }
}
