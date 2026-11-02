import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "trash.js" as Trash

// The command row while a window shows the trash, after Windows' Recycle
// Bin: Empty trash up front, Restore (the picked items, or all of them),
// Delete for the picked ones, then Sort and View.
Rectangle {
    id: row

    required property TrashModel model

    signal emptyRequested
    signal restoreRequested
    signal eraseRequested

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
            icon: Icons.TRASH
            label: qsTr("Empty trash")
            raised: enabled
            enabled: row.model.rows.count > 0
            onClicked: row.emptyRequested()
        }
        ToolButton {
            icon: Icons.RESTORE
            label: Trash.restoreLabel(row.model.selectedCount)
            enabled: row.model.rows.count > 0
            onClicked: row.restoreRequested()
        }
        Separator {}
        ToolButton {
            icon: Icons.TRASH
            enabled: row.model.selectedCount > 0
            onClicked: row.eraseRequested()
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
