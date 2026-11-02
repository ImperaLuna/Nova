pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../theme"
import "../theme/icons.js" as Icons
import "paths.js" as Paths
import "trash.js" as Trash

// Nova Files' top row: back, forward and up, the address bar showing
// where the window is (a part per folder from Home or the root, hidden
// folders dimmer, the current one bold), and the search field.
//
// A click on a part goes there. A click on the bar's empty end (or Ctrl+L,
// Alt+D) turns it into a text field holding the folder's path: Enter goes
// to what's typed, Esc or clicking away puts the parts back. A path that
// isn't a folder keeps the field open, outlined in the error color.
Rectangle {
    id: row

    required property Navigator navigator
    readonly property var parts: Trash.isTrash(navigator.folder) ? Trash.crumbs() : Paths.crumbs(navigator.folder, Quickshell.env("HOME") ?? "", qsTr("Home"))
    property bool editing: false
    property bool wrong: false

    // Turns the address bar into a text field with the path selected.
    function editAddress() {
        field.text = navigator.folder;
        wrong = false;
        editing = true;
        field.selectAll();
        field.forceActiveFocus();
    }

    function submit() {
        row.navigator.visit(field.text, ok => {
            if (ok)
                row.editing = false;
            row.wrong = !ok;
        });
    }

    implicitHeight: Tokens.filesFieldHeight + 2 * Tokens.filesRowPadY
    color: Tokens.surfaceRaised

    Row {
        id: buttons
        anchors.left: parent.left
        anchors.leftMargin: Tokens.rowInset
        anchors.verticalCenter: parent.verticalCenter
        spacing: Tokens.barGap

        ToolButton {
            icon: Icons.LEFT
            enabled: row.navigator.canGoBack
            onClicked: row.navigator.step(-1)
        }
        ToolButton {
            icon: Icons.RIGHT
            enabled: row.navigator.canGoForward
            onClicked: row.navigator.step(1)
        }
        ToolButton {
            icon: Icons.UP
            enabled: row.navigator.canGoUp
            onClicked: row.navigator.up()
        }
    }

    Rectangle {
        id: address
        anchors.left: buttons.right
        anchors.right: search.left
        anchors.leftMargin: Tokens.gap
        anchors.rightMargin: Tokens.barGap
        anchors.verticalCenter: parent.verticalCenter
        height: Tokens.filesFieldHeight
        radius: Tokens.radiusControl
        color: Tokens.filesField
        border.width: row.editing ? Tokens.filesFocusRing : 0
        border.color: row.wrong ? Tokens.error : Tokens.primary
        clip: true

        // The empty end: a click there types a path.
        Item {
            anchors.left: crumbs.right
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            visible: !row.editing

            TapHandler {
                onTapped: row.editAddress()
            }
        }

        Row {
            id: crumbs
            // Ends at the bar's end when the path is longer than the bar,
            // so the current folder always shows.
            x: Math.min(Tokens.iconGap, address.width - Tokens.iconGap - width)
            anchors.verticalCenter: parent.verticalCenter
            visible: !row.editing
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
                    Rectangle {
                        anchors.verticalCenter: parent.verticalCenter
                        width: label.implicitWidth
                        height: label.implicitHeight + 2 * Tokens.filesCrumbPadY
                        radius: Tokens.filesCrumbRadius
                        color: partHover.hovered ? Tokens.hover : "transparent"

                        Behavior on color {
                            ColorAnimation {
                                duration: Tokens.durationFast
                            }
                        }

                        Text {
                            id: label
                            anchors.centerIn: parent
                            leftPadding: Tokens.iconGap
                            rightPadding: Tokens.iconGap
                            font.family: Tokens.fontSans
                            font.pixelSize: Tokens.fontSize
                            font.weight: crumb.current ? Tokens.weightTitle : Tokens.weightBody
                            color: crumb.modelData.hidden ? Tokens.fgDim : Tokens.fg
                            textFormat: Text.PlainText
                            text: crumb.modelData.name
                        }
                        HoverHandler {
                            id: partHover
                        }
                        TapHandler {
                            onTapped: row.navigator.visit(crumb.modelData.path, null)
                        }
                    }
                }
            }
        }

        TextInput {
            id: field
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.leftMargin: Tokens.rowInset
            anchors.rightMargin: Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            visible: row.editing
            clip: true
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            color: Tokens.fg
            selectionColor: Tokens.secondary
            selectedTextColor: Tokens.onAccent
            onTextEdited: row.wrong = false
            onActiveFocusChanged: {
                if (!activeFocus)
                    row.editing = false;
            }
            Keys.onReturnPressed: row.submit()
            Keys.onEnterPressed: row.submit()
            Keys.onEscapePressed: row.editing = false
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
