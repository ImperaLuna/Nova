import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// A button of Nova Files' navigation and command rows: an icon, and for
// the ones that open a menu a label and a small chevron. Dimmed while it
// can't do anything yet.
Rectangle {
    id: button

    required property string icon
    property string label: ""
    property bool menu: false
    // Drawn raised, like New, the row's first action.
    property bool raised: false
    signal clicked

    implicitWidth: label === "" ? Tokens.filesButtonSize : content.implicitWidth + 2 * Tokens.rowInset
    implicitHeight: Tokens.filesButtonSize
    radius: Tokens.radiusControl
    opacity: enabled ? 1 : Tokens.dimOpacity
    color: hover.hovered && enabled ? Tokens.hover : raised ? Tokens.surfaceRaised : "transparent"

    Behavior on color {
        ColorAnimation {
            duration: Tokens.durationFast
        }
    }

    Row {
        id: content
        anchors.centerIn: parent
        spacing: Tokens.iconGap

        LineIcon {
            anchors.verticalCenter: parent.verticalCenter
            path: button.icon
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            visible: button.label !== ""
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            color: Tokens.fg
            text: button.label
        }
        LineIcon {
            anchors.verticalCenter: parent.verticalCenter
            visible: button.menu
            size: Tokens.iconSizeMini
            opacity: Tokens.filesChevronOpacity
            path: Icons.CHEVRON_DOWN
        }
    }

    HoverHandler {
        id: hover
    }
    TapHandler {
        onTapped: button.clicked()
    }
}
