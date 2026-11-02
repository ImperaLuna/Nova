pragma ComponentBehavior: Bound

import QtQuick
import "../theme"

// Nova Files' file list: the column header (Name, then Modified, Type and
// Size at their widths) over the folder's rows, on glass or, as `solid`
// (0..1) rises, the theme's solid list surface.
Rectangle {
    id: list

    property real solid: 0
    readonly property var columns: [qsTr("Modified"), qsTr("Type"), qsTr("Size")]

    color: Tokens.alpha(Tokens.filesSolidList, solid)

    component HeaderLabel: Text {
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeSmall
        font.weight: Tokens.weightLabel
        color: Tokens.filesFaint
        elide: Text.ElideRight
    }

    Row {
        id: header
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Tokens.rowInset
        anchors.topMargin: Tokens.filesRowPadY
        height: Tokens.filesHeaderHeight
        spacing: Tokens.rowInset

        HeaderLabel {
            anchors.verticalCenter: parent.verticalCenter
            width: header.width - Tokens.filesColumns.reduce((sum, column) => sum + column + header.spacing, 0)
            leftPadding: Tokens.rowInset
            text: qsTr("Name")
        }
        Repeater {
            model: list.columns

            HeaderLabel {
                required property string modelData
                required property int index

                anchors.verticalCenter: parent.verticalCenter
                width: Tokens.filesColumns[index]
                horizontalAlignment: index === list.columns.length - 1 ? Text.AlignRight : Text.AlignLeft
                rightPadding: index === list.columns.length - 1 ? Tokens.rowInset : 0
                text: modelData
            }
        }
    }
}
