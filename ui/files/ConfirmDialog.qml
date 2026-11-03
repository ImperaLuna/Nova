pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "paths.js" as Paths
import "listing.js" as Listing
import "trash.js" as Trash

// The dialog in the middle of a Nova Files window that asks before rows
// are deleted for good (Shift+Delete, Delete in the trash, Empty trash),
// after Windows' "Delete file" and "Delete multiple items": the title, the
// question, the one item with its type, size and date when there's one,
// then Yes (the default) and No. Enter says yes, Escape no.
Item {
    id: dialog

    // {title, question, item} (Trash.confirm).
    required property var asking

    signal accepted
    signal declined

    implicitWidth: Tokens.filesConfirmWidth
    implicitHeight: content.implicitHeight + Tokens.panelPad + Tokens.panelPadBottom

    Keys.onReturnPressed: accepted()
    Keys.onEnterPressed: accepted()
    Keys.onEscapePressed: declined()

    GlassShadow {
        glass: surface
    }
    Glass {
        id: surface
        anchors.fill: parent
        radius: Tokens.radiusPanel
    }

    Column {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Tokens.panelPad
        spacing: Tokens.pad

        Text {
            width: parent.width
            text: dialog.asking.title
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeTitle
            font.weight: Tokens.weightTitle
            color: Tokens.fg
        }
        Text {
            width: parent.width
            text: dialog.asking.question
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            color: Tokens.fgDim
        }
        Row {
            visible: dialog.asking.item !== null
            spacing: Tokens.pad

            LineIcon {
                path: dialog.asking.item ? Listing.icon(dialog.asking.item) : ""
                size: Tokens.filesClashIconSize
                color: dialog.asking.item?.dir ? Tokens.filesFolder : Tokens.fgDim
            }
            Column {
                spacing: Tokens.labelNudge * 2

                Text {
                    width: content.width - Tokens.filesClashIconSize - Tokens.pad
                    text: Paths.shown(dialog.asking.item?.name ?? "")
                    textFormat: Text.PlainText
                    elide: Text.ElideMiddle
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSize
                    font.weight: Tokens.weightTitle
                    color: Tokens.fg
                }
                Repeater {
                    model: dialog.asking.item ? Trash.details(dialog.asking.item) : []

                    Text {
                        required property string modelData

                        text: modelData
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        color: Tokens.fgDim
                    }
                }
            }
        }
        Row {
            anchors.right: parent.right
            spacing: Tokens.gap

            DialogButton {
                text: qsTr("Yes")
                accent: true
                minWidth: Tokens.filesDialogButtonMinWidth
                onClicked: dialog.accepted()
            }
            DialogButton {
                text: qsTr("No")
                minWidth: Tokens.filesDialogButtonMinWidth
                onClicked: dialog.declined()
            }
        }
    }
}
