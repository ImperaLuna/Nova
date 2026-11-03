pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "picker.js" as Picker
import "../daemon/protocol.js" as Protocol

// The bar under a Nova Files window in picker mode, as in Windows' Open and
// Save As dialogs: the name field (saving one file), the app's file type
// filters as a button whose menu opens upward, then the accept button (the
// accent, off while there's nothing to pick) and Cancel. Enter in the
// field accepts, Escape anywhere in the window cancels. It fades into the
// window frame as `solid` rises.
Rectangle {
    id: bar

    // novad's picker: {id, mode, title, accept, multiple, folders, name,
    // filters, filter}.
    required property var picker
    // The filter shown, an index into picker.filters.
    property int filterIndex: 0
    // The accept button can pick (Picker.choice isn't "none").
    property bool canAccept: false
    property real solid: 0
    readonly property bool naming: picker.mode === Protocol.PICK_SAVE
    readonly property alias name: nameField.text

    signal accepted
    signal cancelled
    // The name field changed by typing, not by a row picked.
    signal nameEdited

    function setName(name: string) {
        nameField.text = name;
    }

    // Puts the cursor in the name field with the name's stem selected.
    function focusName() {
        nameField.forceActiveFocus();
        nameField.select(0, Picker.stemLength(nameField.text));
    }

    implicitHeight: Tokens.filesFieldHeight + 2 * Tokens.filesRowPadY + Tokens.gap
    color: Tokens.alpha(Tokens.surfaceRaised, Tokens.raisedOpacity * (1 - solid))

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: Tokens.dividerWidth
        color: Tokens.filesRule
    }

    Rectangle {
        id: nameBox
        anchors.left: parent.left
        anchors.right: filterButton.visible ? filterButton.left : answers.left
        anchors.leftMargin: Tokens.rowInset
        anchors.rightMargin: Tokens.gap
        anchors.verticalCenter: parent.verticalCenter
        visible: bar.naming
        height: Tokens.filesFieldHeight
        radius: Tokens.radiusControl
        color: Tokens.filesField
        border.width: nameField.activeFocus ? Tokens.filesFocusRing : 0
        border.color: Tokens.primary

        TextInput {
            id: nameField
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.leftMargin: Tokens.rowInset
            anchors.rightMargin: Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            clip: true
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            color: Tokens.fg
            selectionColor: Tokens.secondary
            selectedTextColor: Tokens.onAccent
            text: bar.picker.name ?? ""
            onTextEdited: bar.nameEdited()
            Keys.onReturnPressed: bar.accepted()
            Keys.onEnterPressed: bar.accepted()
        }
    }

    Rectangle {
        id: filterButton
        anchors.right: answers.left
        anchors.rightMargin: Tokens.gap
        anchors.verticalCenter: parent.verticalCenter
        visible: (bar.picker.filters ?? []).length > 0
        width: Tokens.filesPickerFilterWidth
        height: Tokens.filesFieldHeight
        radius: Tokens.radiusControl
        color: filterHover.hovered || menu.visible ? Tokens.hover : Tokens.filesField

        Text {
            anchors.left: parent.left
            anchors.right: chevron.left
            anchors.leftMargin: Tokens.rowInset
            anchors.rightMargin: Tokens.labelNudge
            anchors.verticalCenter: parent.verticalCenter
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            color: Tokens.fg
            elide: Text.ElideRight
            textFormat: Text.PlainText
            text: Picker.filter(bar.picker, bar.filterIndex)?.name ?? ""
        }
        LineIcon {
            id: chevron
            anchors.right: parent.right
            anchors.rightMargin: Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            size: Tokens.iconSizeMini
            color: Tokens.filesFaint
            rotation: menu.visible ? 180 : 0
            path: Icons.CHEVRON_DOWN
        }
        HoverHandler {
            id: filterHover
        }
        TapHandler {
            onTapped: menu.visible = !menu.visible
        }
    }

    Row {
        id: answers
        anchors.right: parent.right
        anchors.rightMargin: Tokens.rowInset
        anchors.verticalCenter: parent.verticalCenter
        spacing: Tokens.gap

        DialogButton {
            text: Picker.acceptLabel(bar.picker)
            accent: bar.canAccept
            enabled: bar.canAccept
            opacity: bar.canAccept ? 1 : Tokens.filesHiddenIconOpacity
            minWidth: Tokens.filesDialogButtonMinWidth
            onClicked: bar.accepted()
        }
        DialogButton {
            text: qsTr("Cancel")
            minWidth: Tokens.filesDialogButtonMinWidth
            onClicked: bar.cancelled()
        }
    }

    // The file type menu, opening upward from its button.
    Rectangle {
        id: menu
        visible: false
        x: filterButton.x
        y: filterButton.y - height - Tokens.labelNudge
        z: 1
        width: filterButton.width
        height: rows.implicitHeight + 2 * Tokens.menuPad
        radius: Tokens.radiusCard
        color: Tokens.bg
        border.width: Tokens.dividerWidth
        border.color: Tokens.glassRim

        Column {
            id: rows
            x: Tokens.menuPad
            y: Tokens.menuPad
            width: parent.width - 2 * x

            Repeater {
                model: bar.picker.filters ?? []

                Rectangle {
                    id: option

                    required property var modelData
                    required property int index

                    width: rows.width
                    height: Tokens.menuRowHeight
                    radius: Tokens.menuRowRadius
                    color: optionHover.hovered ? Tokens.selected : "transparent"

                    LineIcon {
                        id: check
                        anchors.left: parent.left
                        anchors.leftMargin: Tokens.rowInset
                        anchors.verticalCenter: parent.verticalCenter
                        size: Tokens.iconSizeMini
                        color: Tokens.primary
                        visible: option.index === bar.filterIndex
                        path: Icons.CHECK
                    }
                    Text {
                        anchors.left: check.right
                        anchors.right: parent.right
                        anchors.leftMargin: Tokens.labelNudge * 2
                        anchors.rightMargin: Tokens.rowInset
                        anchors.verticalCenter: parent.verticalCenter
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        font.weight: option.index === bar.filterIndex ? Tokens.weightTitle : Tokens.weightBody
                        color: Tokens.fg
                        elide: Text.ElideRight
                        textFormat: Text.PlainText
                        text: option.modelData.name
                    }
                    HoverHandler {
                        id: optionHover
                    }
                    TapHandler {
                        onTapped: {
                            bar.filterIndex = option.index;
                            menu.visible = false;
                        }
                    }
                }
            }
        }
    }
}
