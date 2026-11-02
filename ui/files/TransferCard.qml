import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "transfers.js" as Transfers

// One copy or move job's card at the bottom right of a Nova Files window:
// what it copies, a bar and the percent, and a button that cancels it.
// The bar goes quiet while the job waits on a name clash. A job that
// failed shows why in place of the bar, and the button dismisses it.
Item {
    id: card

    required property var transfer
    readonly property bool failed: transfer.state === Protocol.TRANSFER_FAILED
    readonly property bool paused: transfer.state === Protocol.TRANSFER_ASKING

    signal cancelled
    signal dismissed

    implicitWidth: Tokens.filesJobWidth
    implicitHeight: row.implicitHeight + 2 * Tokens.pad

    GlassShadow {
        glass: surface
    }
    Glass {
        id: surface
        anchors.fill: parent
        radius: Tokens.radiusCard
    }

    Row {
        id: row
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: Tokens.pad
        anchors.rightMargin: Tokens.gap
        spacing: Tokens.rowInset

        LineIcon {
            anchors.verticalCenter: parent.verticalCenter
            path: card.transfer.mode === Protocol.TRANSFER_MOVE ? Icons.CUT : Icons.COPY
            color: card.failed ? Tokens.error : Tokens.fgDim
        }
        Column {
            anchors.verticalCenter: parent.verticalCenter
            width: row.width - x - percent.width - close.width - 2 * row.spacing
            spacing: Tokens.iconGap

            Text {
                width: parent.width
                text: Transfers.label(card.transfer)
                textFormat: Text.PlainText
                elide: Text.ElideMiddle
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: Tokens.weightLabel
                color: Tokens.fg
            }
            Rectangle {
                visible: !card.failed
                width: parent.width
                height: Tokens.meterHeightSmall
                radius: height / 2
                color: Tokens.track

                Rectangle {
                    width: parent.width * Transfers.fraction(card.transfer)
                    height: parent.height
                    radius: parent.radius
                    color: card.paused ? Tokens.fgDim : Tokens.primary

                    Behavior on width {
                        NumberAnimation {
                            duration: Tokens.durationFill
                            easing.type: Easing.BezierSpline
                            easing.bezierCurve: Tokens.easing
                        }
                    }
                }
            }
            Text {
                visible: card.failed
                width: parent.width
                text: card.transfer.error ?? ""
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                maximumLineCount: Tokens.failedCardLines
                elide: Text.ElideRight
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                color: Tokens.error
            }
        }
        Text {
            id: percent
            anchors.verticalCenter: parent.verticalCenter
            visible: !card.failed
            width: visible ? implicitWidth : 0
            text: Transfers.percent(card.transfer)
            font.family: Tokens.fontMono
            font.pixelSize: Tokens.fontSizeSmall
            font.weight: Tokens.weightLabel
            color: Tokens.fgDim
        }
        ToolButton {
            id: close
            anchors.verticalCenter: parent.verticalCenter
            icon: Icons.CLOSE
            onClicked: card.failed ? card.dismissed() : card.cancelled()
        }
    }
}
