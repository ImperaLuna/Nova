import QtQuick
import "../theme"

// A round answer button of Nova Files' dialogs: quiet, raised, or the
// accent for the default answer.
Rectangle {
    id: answer

    required property string text
    property bool quiet: false
    property bool accent: false
    // At least this wide, so short answers (Yes, No) match.
    property real minWidth: 0
    signal clicked

    width: Math.max(label.implicitWidth + 2 * Tokens.pad, minWidth)
    height: Tokens.filesClashButtonHeight
    radius: height / 2
    color: accent ? Tokens.primary : hover.hovered ? Tokens.selected : quiet ? "transparent" : Tokens.surfaceRaised

    Behavior on color {
        ColorAnimation {
            duration: Tokens.durationFast
        }
    }

    Text {
        id: label
        anchors.centerIn: parent
        text: answer.text
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSize
        font.weight: answer.accent ? Tokens.weightTitle : Tokens.weightBody
        color: answer.accent ? Tokens.onAccent : answer.quiet ? Tokens.fgDim : Tokens.fg
    }
    HoverHandler {
        id: hover
    }
    TapHandler {
        onTapped: answer.clicked()
    }
}
