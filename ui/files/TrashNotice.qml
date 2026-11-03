import QtQuick
import "../theme"
import "paths.js" as Paths
import "../theme/icons.js" as Icons
import "listing.js" as Listing

// The notice at the bottom right of a Nova Files window when a trashed
// item can't go back (its place is taken) or the trash refused something:
// the item's icon and name, novad's reason under it, and a button that
// dismisses it.
Item {
    id: notice

    // {row, error}: the item (a trash or file list row) and why.
    required property var problem

    signal dismissed

    implicitWidth: Tokens.filesNoticeWidth
    implicitHeight: content.implicitHeight + 2 * Tokens.pad

    GlassShadow {
        glass: surface
    }
    Glass {
        id: surface
        anchors.fill: parent
        radius: Tokens.radiusCard
    }

    Row {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: Tokens.pad
        anchors.rightMargin: Tokens.gap
        spacing: Tokens.rowInset

        LineIcon {
            path: notice.problem.row ? Listing.icon(notice.problem.row) : Icons.TRASH
            color: Tokens.fgDim
        }
        Column {
            width: content.width - x - close.width - content.spacing
            spacing: Tokens.labelNudge

            Text {
                width: parent.width
                visible: notice.problem.row !== null
                text: Paths.shown(notice.problem.row?.name ?? "")
                textFormat: Text.PlainText
                elide: Text.ElideMiddle
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: Tokens.weightTitle
                color: Tokens.fg
            }
            Text {
                width: parent.width
                text: notice.problem.error
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                maximumLineCount: Tokens.failedCardLines
                elide: Text.ElideRight
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                color: Tokens.fgDim
            }
        }
        ToolButton {
            id: close
            icon: Icons.CLOSE
            onClicked: notice.dismissed()
        }
    }
}
