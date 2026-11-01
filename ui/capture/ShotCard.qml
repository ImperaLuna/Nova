pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../daemon/protocol.js" as Protocol
import "capture.js" as Capture
import "../text/files.js" as Files
import "../text/duration.js" as Duration

// The card a saved screenshot lands in: the shot, that it was copied, where
// it went and its size, and Open, Folder and Delete. A recording's card has
// its length where the shot would be, and where it went. A line along the
// bottom drains over Tokens.durationShotCard, and the card leaves when it
// runs out; the pointer on the card holds it. A recording that didn't start
// shows here too: why, with Try again and Open log, for longer.
Item {
    id: card

    // Daemon's screenshotSaved shot: {id, path, width, height, copied}; or
    // with `video`, recordingSaved's {id, path, duration_ms}, or {reason}
    // when the recording didn't start.
    required property var shot
    property bool video: false
    readonly property bool failed: video && typeof shot?.reason === "string"

    readonly property bool held: hover.hovered
    // A Protocol.SHOT_* button was clicked, or Capture.CARD_* when failed.
    signal actionRequested(string action)
    // Its time ran out.
    signal expired

    // The share of its time still left, 1 to 0.
    readonly property alias remaining: timer.remaining

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
                visible: !card.failed
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
                        name: card.failed ? Capture.ICON_VIDEO_OFF : Capture.ICON_CHECK
                        color: card.failed ? Tokens.error : Tokens.success
                    }
                    // Saved either way; copied unless the clipboard failed.
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        text: card.failed ? qsTr("Recording didn't start") : card.video ? qsTr("Recording saved") : card.shot?.copied ? qsTr("Copied to clipboard") : qsTr("Saved")
                        textFormat: Text.PlainText
                        color: Tokens.fg
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        font.weight: Tokens.weightTitle
                    }
                }

                // novad's reason, as many lines as it takes up to the most.
                Text {
                    visible: card.failed
                    width: parent.width
                    wrapMode: Text.Wrap
                    maximumLineCount: Tokens.failedCardLines
                    elide: Text.ElideRight
                    text: card.failed ? card.shot.reason : ""
                    textFormat: Text.PlainText
                    color: Tokens.fgDim
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSmall
                }

                Text {
                    visible: !card.failed
                    width: parent.width
                    elide: Text.ElideMiddle
                    text: {
                        if (!card.shot || card.failed)
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
                readonly property real buttonWidth: (width - (actions.length - 1) * spacing) / actions.length
                readonly property var actions: card.failed ? [
                    {
                        action: Capture.CARD_RETRY,
                        label: qsTr("Try again")
                    },
                    {
                        action: Capture.CARD_LOG,
                        label: qsTr("Open log")
                    }
                ] : [
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

                Repeater {
                    model: buttons.actions
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
            color: card.failed ? Tokens.error : Tokens.primary
            opacity: Tokens.drainOpacity
            duration: card.failed ? Tokens.durationFailedCard : Tokens.durationShotCard
            held: card.held
            onExpired: card.expired()
        }
    }
}
