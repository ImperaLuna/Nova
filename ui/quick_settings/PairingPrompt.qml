pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "bluetooth.js" as Bluetooth

// A pairing question (a bluetooth event's `pairing`) about the device
// called `name`: the question, the code to compare or type there, or a
// field for a PIN, and the buttons. Under the device's row on the
// Bluetooth page, and on the pop-up when a phone pairs with this computer.
Rectangle {
    id: prompt

    required property var request
    required property string name
    // What was typed in the field before this prompt was made (its row
    // moved), and each edit, for the next one.
    property string draft: ""

    // "accept", "reject" or {code}, for BluetoothService.answer.
    signal answered(var answer)
    signal drafted(string text)

    readonly property var ask: request.ask
    readonly property string code: Bluetooth.code(ask)
    readonly property bool typed: Bluetooth.typed(ask)
    readonly property var labels: Bluetooth.buttons(ask, request.incoming)
    // A row reused for another device (rows above it went) picks up what
    // was typed, as a new one does.
    onRequestChanged: field.text = draft
    readonly property bool canAccept: !typed || Bluetooth.validCode(ask, field.text)

    function accept() {
        if (!canAccept)
            return;
        answered(typed ? {
            code: field.text.trim()
        } : "accept");
    }

    implicitHeight: content.implicitHeight + 2 * Tokens.pad
    radius: Tokens.radiusRow
    color: Tokens.alpha(Tokens.primary, Tokens.pairingTintOpacity)

    Column {
        id: content
        x: Tokens.pad
        y: Tokens.pad
        width: parent.width - 2 * Tokens.pad
        spacing: Tokens.gap

        Text {
            width: parent.width
            wrapMode: Text.Wrap
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeSmall
            color: Tokens.fg
            textFormat: Text.PlainText
            text: Bluetooth.question(prompt.ask, prompt.name, prompt.request.incoming)
        }

        // The code, a cell per character; the ones already typed on the
        // device lit.
        Row {
            objectName: "code"
            visible: prompt.code !== ""
            spacing: Tokens.pairingCellGap

            Repeater {
                model: prompt.code.length

                Rectangle {
                    id: cell
                    required property int index
                    readonly property bool done: prompt.ask.kind === "show_passkey" && index < prompt.ask.typed
                    width: Tokens.pairingCellWidth
                    height: Tokens.pairingCellHeight
                    radius: Tokens.radiusSegment
                    color: done ? Tokens.primary : Tokens.track

                    Text {
                        anchors.centerIn: parent
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSizePairingCode
                        font.weight: Font.DemiBold
                        color: cell.done ? Tokens.onAccent : Tokens.fg
                        text: prompt.code.charAt(cell.index)
                    }
                }
            }
        }

        Rectangle {
            visible: prompt.typed
            width: parent.width
            height: Tokens.pairingFieldHeight
            radius: Tokens.radiusSegment
            color: Tokens.track

            TextInput {
                id: field
                objectName: "field"
                anchors.verticalCenter: parent.verticalCenter
                x: Tokens.pad
                width: parent.width - 2 * Tokens.pad
                clip: true
                focus: prompt.typed
                maximumLength: prompt.ask.kind === "enter_passkey" ? 6 : 16
                inputMethodHints: prompt.ask.kind === "enter_passkey" ? Qt.ImhDigitsOnly : Qt.ImhNone
                font.family: Tokens.fontMono
                font.pixelSize: Tokens.fontSize
                color: Tokens.fg
                selectionColor: Tokens.secondary
                selectedTextColor: Tokens.onAccent
                onAccepted: prompt.accept()
                onTextEdited: prompt.drafted(text)
                Component.onCompleted: text = prompt.draft

                Text {
                    visible: field.text === ""
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSize
                    color: Tokens.fgDim
                    text: prompt.ask.kind === "enter_passkey" ? qsTr("Code") : qsTr("PIN")
                }
            }
        }

        Text {
            visible: prompt.ask.kind === "enter_pin"
            width: parent.width
            wrapMode: Text.Wrap
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeTiny
            color: Tokens.fgDim
            text: qsTr("It's in the device's manual. Many use 0000.")
        }

        Row {
            anchors.right: parent.right
            spacing: Tokens.iconGap

            PromptButton {
                objectName: "decline"
                text: prompt.labels[0]
                onClicked: prompt.answered("reject")
            }
            PromptButton {
                objectName: "accept"
                visible: prompt.labels[1] !== ""
                text: prompt.labels[1]
                accent: true
                enabled: prompt.canAccept
                onClicked: prompt.accept()
            }
        }
    }
}
