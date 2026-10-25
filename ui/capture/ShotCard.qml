pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../daemon/protocol.js" as Protocol
import "capture.js" as Capture

// The card a saved screenshot lands in: the shot, that it was copied, where
// it went and its size, and Open, Folder and Delete. A line along the
// bottom drains over Tokens.durationShotCard, and the card leaves when it's
// empty; the pointer on the card holds it.
Item {
    id: card

    // Daemon's screenshotSaved shot: {id, path, width, height, copied}.
    required property var shot

    readonly property bool held: hover.hovered
    // A Protocol.SHOT_* button was clicked.
    signal actionRequested(string action)
    // The drain ran out.
    signal expired

    // The share of the time left, 1 to 0.
    property real remaining

    implicitWidth: Tokens.shotCardWidth
    implicitHeight: content.implicitHeight + 2 * Tokens.pad + Tokens.gap

    // Starts the drain again (a new shot in the same card).
    function restart() {
        drain.restart();
    }

    NumberAnimation on remaining {
        id: drain
        from: 1
        to: 0
        duration: Tokens.durationShotCard
        paused: card.held && running
        onFinished: card.expired()
    }

    HoverHandler {
        id: hover
    }

    GlassShadow {
        glass: surface
    }

    Glass {
        id: surface
        anchors.fill: parent
        radius: Tokens.radiusPanel
        clip: true

        Column {
            id: content
            anchors.fill: parent
            anchors.margins: Tokens.pad + Tokens.gap / 2
            spacing: Tokens.shotCardGap

            Rectangle {
                width: parent.width
                height: Tokens.shotThumbHeight
                radius: Tokens.radiusRow
                color: Tokens.backdrop
                clip: true

                Image {
                    anchors.fill: parent
                    source: card.shot ? Capture.fileUrl(card.shot.path) : ""
                    // Decoded at the card's size, not the shot's.
                    sourceSize.width: 2 * width
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                    cache: false
                    smooth: true
                }
            }

            Column {
                width: parent.width
                spacing: Tokens.labelNudge

                Row {
                    spacing: Tokens.iconGap
                    CaptureIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        name: Capture.ICON_CHECK
                        color: Tokens.success
                    }
                    // Saved either way; copied unless the clipboard failed.
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        text: card.shot?.copied ? qsTr("Copied to clipboard") : qsTr("Saved")
                        textFormat: Text.PlainText
                        color: Tokens.fg
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        font.weight: Font.DemiBold
                    }
                }

                Text {
                    width: parent.width
                    elide: Text.ElideMiddle
                    text: card.shot ? `${Capture.folderLabel(card.shot.path)} · ${Capture.size(card.shot.width, card.shot.height)} · PNG` : ""
                    textFormat: Text.PlainText
                    color: Tokens.fgDim
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSmall
                }
            }

            Row {
                id: buttons
                width: parent.width
                spacing: Tokens.iconGap
                readonly property real buttonWidth: (width - 2 * spacing) / 3

                Repeater {
                    model: [
                        {
                            action: Protocol.SHOT_OPEN,
                            label: qsTr("Open")
                        },
                        {
                            action: Protocol.SHOT_FOLDER,
                            label: qsTr("Folder")
                        },
                        {
                            action: Protocol.SHOT_DELETE,
                            label: qsTr("Delete")
                        }
                    ]
                    CaptureButton {
                        required property var modelData
                        width: buttons.buttonWidth
                        buttonHeight: Tokens.shotButtonHeight
                        icon: modelData.action
                        label: modelData.label
                        color: hovered ? Tokens.hover : Tokens.surfaceRaised
                        tint: modelData.action === Protocol.SHOT_DELETE && hovered ? Tokens.error : Tokens.fg
                        onClicked: card.actionRequested(modelData.action)
                    }
                }
            }
        }

        Rectangle {
            anchors.left: parent.left
            anchors.bottom: parent.bottom
            width: parent.width * card.remaining
            height: Tokens.drainHeight
            color: Tokens.primary
            opacity: Tokens.drainOpacity
        }
    }
}
