import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// Nova Files' command row, under the navigation row: New first, then cut,
// copy, paste, rename and delete, then Sort and View.
Rectangle {
    id: row

    implicitHeight: actions.implicitHeight + Tokens.filesCommandPadTop + Tokens.gap
    color: Tokens.surfaceRaised

    component Separator: Rectangle {
        anchors.verticalCenter: parent?.verticalCenter
        width: Tokens.dividerWidth
        height: Tokens.filesSeparatorHeight
        color: Tokens.glassRim
    }

    Row {
        id: actions
        anchors.left: parent.left
        anchors.leftMargin: Tokens.rowInset
        anchors.top: parent.top
        anchors.topMargin: Tokens.filesCommandPadTop
        spacing: Tokens.labelNudge * 2

        ToolButton {
            icon: Icons.PLUS
            label: qsTr("New")
            menu: true
            raised: true
            enabled: false
        }
        Separator {}
        ToolButton {
            icon: Icons.CUT
            enabled: false
        }
        ToolButton {
            icon: Icons.COPY
            enabled: false
        }
        ToolButton {
            icon: Icons.PASTE
            enabled: false
        }
        ToolButton {
            icon: Icons.RENAME
            enabled: false
        }
        ToolButton {
            icon: Icons.TRASH
            enabled: false
        }
        Separator {}
        ToolButton {
            icon: Icons.SORT
            label: qsTr("Sort")
            menu: true
            enabled: false
        }
        ToolButton {
            icon: Icons.VIEW
            label: qsTr("View")
            menu: true
            enabled: false
        }
    }
}
