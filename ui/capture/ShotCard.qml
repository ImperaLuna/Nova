pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../daemon/protocol.js" as Protocol
import "capture.js" as Capture
import "../text/files.js" as Files
import "../text/duration.js" as Duration

// The card a saved screenshot lands in: the shot, that it was copied, where
// it went and its size, and Open, Folder and Delete. A recording's card has
// its length where the shot would be, and where it went. A line of pieces
// along the bottom goes out over Tokens.durationShotCard, and the card
// leaves when the last one goes; the pointer on the card holds it.
Item {
    id: card

    // Daemon's screenshotSaved shot: {id, path, width, height, copied}; or
    // with `video`, recordingSaved's {id, path, duration_ms}.
    required property var shot
    property bool video: false

    readonly property bool held: hover.hovered
    // A Protocol.SHOT_* button was clicked.
    signal actionRequested(string action)
    // Its time ran out.
    signal expired

    // Pieces of its time still left.
    readonly property alias lit: timer.lit

    implicitWidth: Tokens.shotCardWidth
    implicitHeight: content.implicitHeight + 2 * Tokens.pad + Tokens.gap

    // Its time starts over (a new shot in the same card).
    function restart() {
        timer.restart();
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
                    visible: !card.video
                    anchors.fill: parent
                    source: card.shot && !card.video ? Files.fileUrl(card.shot.path) : ""
                    // Decoded at the card's size, not the shot's.
                    sourceSize.width: 2 * width
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                    cache: false
                    smooth: true
                }

                Column {
                    visible: card.video
                    anchors.centerIn: parent
                    spacing: Tokens.iconGap

                    CaptureIcon {
                        anchors.horizontalCenter: parent.horizontalCenter
                        name: Capture.ICON_VIDEO
                        color: Tokens.fgDim
                    }
                    Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: card.video && card.shot ? Duration.clock(card.shot.duration_ms) : ""
                        textFormat: Text.PlainText
                        color: Tokens.fg
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSize
                    }
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
                        text: card.video ? qsTr("Recording saved") : card.shot?.copied ? qsTr("Copied to clipboard") : qsTr("Saved")
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
                    text: {
                        if (!card.shot)
                            return "";
                        if (card.video)
                            return `${Capture.folderLabel(card.shot.path)} · ${Duration.clock(card.shot.duration_ms)} · MP4`;
                        return `${Capture.folderLabel(card.shot.path)} · ${Capture.size(card.shot.width, card.shot.height)} · PNG`;
                    }
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

        Countdown {
            id: timer
            anchors.left: parent.left
            anchors.bottom: parent.bottom
            width: parent.width
            height: Tokens.drainHeight
            radius: 0
            color: Tokens.primary
            opacity: Tokens.drainOpacity
            duration: Tokens.durationShotCard
            held: card.held
            onExpired: card.expired()
        }
    }
}
