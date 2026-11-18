pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../text/files.js" as Files
import "launcher.js" as LauncherLogic

// One row in the launcher's list, an app or what another provider found:
// its icon, its name (the letters a search matched in the accent) and what
// it is, dimmer. Hovering an app shows a pin button, which stays lit while
// the app is pinned. A file or folder (the Files mode) shows its type's line
// icon and the folder it's in, fainter when it's hidden, and on hover a
// button that shows it in its folder. The selected row shows
// ↵. The list draws the highlight behind it; while the row launches it
// turns the accent and the row's text turns to match.
Item {
    id: row

    // A launcher.js row: {provider, id, name, runs, description, icon, file}.
    required property var app
    // A file or folder: novad's LauncherFile for it, else null.
    readonly property var file: app.file ?? null
    property string iconSource: ""
    property bool selected: false
    // Only apps can be pinned.
    property bool pinnable: true
    property bool pinned: false
    // Launching: on the accent.
    property bool launching: false

    // The pointer moved over the row, to `at` on the screen.
    signal hovered(point at)
    signal clicked
    signal pinClicked
    signal showInFolderClicked

    readonly property color ink: launching ? Tokens.onAccent : file?.hidden ? Tokens.launcherFileFaint : Tokens.fg
    readonly property color dim: launching ? Tokens.onAccent : Tokens.fgDim

    implicitHeight: Tokens.launcherRowHeight

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onPositionChanged: event => row.hovered(mapToItem(null, event.x, event.y))
        onClicked: row.clicked()
    }

    Item {
        id: icon
        anchors.verticalCenter: parent.verticalCenter
        x: Tokens.rowInset
        width: Tokens.launcherRowIcon
        height: Tokens.launcherRowIcon

        AppIcon {
            visible: row.file === null
            appId: row.app.id
            name: row.app.name
            source: row.iconSource
        }

        LineIcon {
            anchors.centerIn: parent
            visible: row.file !== null
            size: Tokens.iconSizeMenu
            path: row.file ? LauncherLogic.fileIcon(row.file) : ""
            color: row.launching ? Tokens.onAccent : row.file?.dir ? Tokens.launcherFolder : Tokens.fgDim
            opacity: row.file?.hidden ? Tokens.launcherHiddenIconOpacity : 1
        }
    }

    Row {
        id: label
        objectName: "label"
        anchors.verticalCenter: parent.verticalCenter
        x: icon.x + icon.width + Tokens.pad
        width: end.x - x - Tokens.pad
        spacing: Tokens.gap
        clip: true

        Row {
            id: name
            anchors.verticalCenter: parent.verticalCenter
            Repeater {
                model: row.app.runs
                // Each run takes what's left of the label, so a long name
                // ends in an ellipsis instead of being cut mid-letter.
                delegate: Text {
                    required property var modelData
                    required property int index
                    objectName: `nameRun${index}`
                    width: Math.min(implicitWidth, Math.max(0, label.width - x))
                    elide: Text.ElideRight
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSize
                    font.weight: modelData.matched ? Tokens.weightTitle : Tokens.weightLabel
                    color: modelData.matched && !row.launching ? Tokens.primary : row.ink
                    textFormat: Text.PlainText
                    text: row.file ? Files.shown(modelData.text) : modelData.text
                }
            }
        }

        Text {
            anchors.verticalCenter: parent.verticalCenter
            width: Math.max(0, label.width - name.width - label.spacing)
            visible: text !== ""
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeSmall
            color: row.dim
            elide: Text.ElideRight
            textFormat: Text.PlainText
            text: row.file ? Files.shown(row.app.description) : row.app.description
        }
    }

    Row {
        id: end
        anchors.verticalCenter: parent.verticalCenter
        anchors.right: parent.right
        anchors.rightMargin: Tokens.rowInset
        spacing: Tokens.pad

        Rectangle {
            id: pinButton
            visible: row.pinnable
            width: Tokens.pinButtonSize
            height: Tokens.pinButtonSize
            radius: height / 2
            color: pinMouse.containsMouse ? Tokens.hover : "transparent"
            opacity: !row.launching && (row.pinned || mouse.containsMouse || pinMouse.containsMouse) ? 1 : 0
            Behavior on opacity {
                NumberAnimation {
                    duration: Tokens.durationFast
                }
            }

            LineIcon {
                anchors.centerIn: parent
                size: Tokens.iconSizeSmall
                path: Icons.PIN
                fill: row.pinned ? Icons.PIN : ""
                color: row.pinned ? Tokens.primary : pinMouse.containsMouse ? Tokens.fg : Tokens.fgDim
            }

            MouseArea {
                id: pinMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: row.pinClicked()
            }
        }

        Rectangle {
            id: folderButton
            objectName: "showInFolder"
            visible: row.file !== null
            width: Tokens.pinButtonSize
            height: Tokens.pinButtonSize
            radius: height / 2
            color: folderMouse.containsMouse ? Tokens.hover : "transparent"
            opacity: !row.launching && (mouse.containsMouse || folderMouse.containsMouse) ? 1 : 0
            Behavior on opacity {
                NumberAnimation {
                    duration: Tokens.durationFast
                }
            }

            LineIcon {
                anchors.centerIn: parent
                size: Tokens.iconSizeSmall
                path: Icons.FOLDER
                color: folderMouse.containsMouse ? Tokens.fg : Tokens.fgDim
            }

            MouseArea {
                id: folderMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: row.showInFolderClicked()
            }
        }

        Text {
            anchors.verticalCenter: parent.verticalCenter
            font.family: Tokens.fontMono
            font.pixelSize: Tokens.fontSizeSmall
            color: row.dim
            opacity: row.selected ? 1 : 0
            text: "↵"
        }
    }
}
