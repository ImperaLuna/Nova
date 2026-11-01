import QtQuick
import "../theme"
import "launcher.js" as LauncherLogic

// An app's icon, `size` square: the image `source` names once it loads,
// else the app's first letter on a swatch picked by its id, so a missing
// icon still looks deliberate.
Item {
    id: icon

    required property string appId
    required property string name
    // From LauncherService.iconSource; "" when the app has no icon.
    property string source: ""
    property int size: Tokens.launcherRowIcon

    readonly property bool loaded: image.status === Image.Ready

    implicitWidth: size
    implicitHeight: size

    Image {
        id: image
        anchors.fill: parent
        source: icon.source
        sourceSize: Qt.size(icon.size * Screen.devicePixelRatio, icon.size * Screen.devicePixelRatio)
        fillMode: Image.PreserveAspectFit
        asynchronous: true
        smooth: true
        mipmap: true
        visible: icon.loaded
    }

    Rectangle {
        anchors.fill: parent
        visible: !icon.loaded
        radius: Tokens.letterTileRadius * icon.size / Tokens.launcherRowIcon
        color: Tokens[`swatch${LauncherLogic.swatch(icon.appId)}`]

        Text {
            anchors.centerIn: parent
            font.family: Tokens.fontSans
            font.pixelSize: Math.round(icon.size * Tokens.letterScale)
            font.weight: Tokens.weightTitle
            color: Tokens.onAccent
            textFormat: Text.PlainText
            text: LauncherLogic.letter(icon.name)
        }
    }
}
