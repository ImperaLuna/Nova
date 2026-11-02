pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "transfers.js" as Transfers

// The name clash dialog in the middle of a Nova Files window, after COSMIC
// Files': the name that's taken, the one there now above the one that
// would take its place (each with its size or contents and when it was last
// changed), "Apply to all" while more clashes like it wait, then Cancel,
// Skip and the default answer: Replace for a file, Merge for a folder.
// Enter gives the default, Escape cancels the job.
Item {
    id: dialog

    required property var clash
    property bool all: false

    // `answer` is a Protocol.CLASH_* answer; `all` passes it on to the
    // other clashes like this one.
    signal answered(string answer, bool all)
    signal cancelled

    implicitWidth: Tokens.filesClashWidth
    implicitHeight: content.implicitHeight + Tokens.panelPad + Tokens.panelPadBottom

    // A new clash starts unticked.
    onClashChanged: all = false

    Keys.onReturnPressed: answered(Transfers.defaultAnswer(clash), all)
    Keys.onEnterPressed: answered(Transfers.defaultAnswer(clash), all)
    Keys.onEscapePressed: cancelled()

    component Side: Row {
        id: side

        required property string label
        required property var item

        spacing: Tokens.pad

        LineIcon {
            path: side.item.items !== undefined ? Icons.FOLDER : Icons.DOCUMENT
            size: Tokens.filesClashIconSize
            color: Tokens.fgDim
        }
        Column {
            spacing: Tokens.labelNudge * 2

            Text {
                text: side.label
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: Tokens.weightTitle
                color: Tokens.fg
            }
            Text {
                text: Transfers.amount(side.item)
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                color: Tokens.fgDim
            }
            Text {
                visible: text !== ""
                text: Transfers.modified(side.item)
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                color: Tokens.fgDim
            }
        }
    }

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
            text: Transfers.clashTitle(dialog.clash)
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeTitle
            font.weight: Tokens.weightTitle
            color: Tokens.fg
        }
        Text {
            width: parent.width
            text: Transfers.clashQuestion(dialog.clash)
            wrapMode: Text.Wrap
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            color: Tokens.fgDim
        }
        Side {
            label: Transfers.originalLabel(dialog.clash)
            item: dialog.clash.original
        }
        Side {
            label: Transfers.replacementLabel(dialog.clash)
            item: dialog.clash.replacement
        }
        Row {
            visible: dialog.clash.others > 0
            spacing: Tokens.rowInset

            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: Tokens.menuCheckSize
                height: Tokens.menuCheckSize
                radius: Tokens.menuCheckRadius
                color: dialog.all ? Tokens.primary : "transparent"
                border.width: dialog.all ? 0 : Tokens.strokeThin
                border.color: Tokens.filesFaint

                LineIcon {
                    anchors.centerIn: parent
                    visible: dialog.all
                    path: Icons.CHECK
                    size: Tokens.menuCheckIconSize
                    color: Tokens.onAccent
                }
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: Transfers.allLabel(dialog.clash)
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                color: Tokens.fg
            }
            TapHandler {
                onTapped: dialog.all = !dialog.all
            }
        }
        Item {
            width: parent.width
            height: Tokens.filesClashButtonHeight

            DialogButton {
                anchors.left: parent.left
                text: qsTr("Cancel")
                quiet: true
                onClicked: dialog.cancelled()
            }
            Row {
                anchors.right: parent.right
                spacing: Tokens.gap

                DialogButton {
                    text: qsTr("Skip")
                    onClicked: dialog.answered(Protocol.CLASH_SKIP, dialog.all)
                }
                DialogButton {
                    text: Transfers.defaultLabel(dialog.clash)
                    accent: true
                    onClicked: dialog.answered(Transfers.defaultAnswer(dialog.clash), dialog.all)
                }
            }
        }
    }
}
