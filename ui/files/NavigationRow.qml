pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../theme"
import "../theme/icons.js" as Icons
import "paths.js" as Paths

// Nova Files' top row: back, forward and up, the address bar showing
// where the window is (a part per folder from Home or the root, dot
// folders dimmer, the current one bold), and the search field.
Rectangle {
    id: row

    required property string folder
    readonly property var parts: Paths.crumbs(folder, Quickshell.env("HOME") ?? "", qsTr("Home"))

    implicitHeight: Tokens.filesFieldHeight + 2 * Tokens.filesRowPadY
    // How solid the window frame is (0..1): the row fades into it.
    property real solid: 0

    color: Tokens.alpha(Tokens.surfaceRaised, Tokens.raisedOpacity * (1 - solid))

    Row {
        id: buttons
        anchors.left: parent.left
        anchors.leftMargin: Tokens.rowInset
        anchors.verticalCenter: parent.verticalCenter
        spacing: Tokens.barGap

        ToolButton {
            icon: Icons.LEFT
            enabled: false
        }
        ToolButton {
            icon: Icons.RIGHT
            enabled: false
        }
        ToolButton {
            icon: Icons.UP
            enabled: false
        }
    }

    Rectangle {
        anchors.left: buttons.right
        anchors.right: search.left
        anchors.leftMargin: Tokens.gap
        anchors.rightMargin: Tokens.barGap
        anchors.verticalCenter: parent.verticalCenter
        height: Tokens.filesFieldHeight
        radius: Tokens.radiusControl
        color: Tokens.filesField
        clip: true

        Row {
            anchors.left: parent.left
            anchors.leftMargin: Tokens.iconGap
            anchors.verticalCenter: parent.verticalCenter
            spacing: Tokens.labelNudge

            Repeater {
                model: row.parts

                Row {
                    id: crumb

                    required property var modelData
                    required property int index
                    readonly property bool current: index === row.parts.length - 1

                    anchors.verticalCenter: parent.verticalCenter
                    spacing: Tokens.labelNudge

                    LineIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        visible: crumb.index > 0
                        size: Tokens.iconSizeMini
                        color: Tokens.filesFaint
                        path: Icons.RIGHT
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        leftPadding: Tokens.iconGap
                        rightPadding: Tokens.iconGap
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        font.weight: crumb.current ? Tokens.weightTitle : Tokens.weightBody
                        color: crumb.modelData.hidden ? Tokens.fgDim : Tokens.fg
                        textFormat: Text.PlainText
                        text: crumb.modelData.name
                    }
                }
            }
        }
    }

    Rectangle {
        id: search
        anchors.right: parent.right
        anchors.rightMargin: Tokens.rowInset
        anchors.verticalCenter: parent.verticalCenter
        width: Tokens.filesSearchWidth
        height: Tokens.filesFieldHeight
        radius: Tokens.radiusControl
        color: Tokens.filesField

        LineIcon {
            anchors.left: parent.left
            anchors.leftMargin: Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            color: Tokens.fgDim
            path: Icons.SEARCH
        }
    }
}
