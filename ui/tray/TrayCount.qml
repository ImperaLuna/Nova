import QtQuick
import "../theme"

// How many of the tray's icons are out of sight on one side of the right
// pill's strip, as a "+N". With none out of sight it fades out but keeps
// its room, so the pill keeps its width as the strip scrolls. Its own
// opacity is left to the pill, which reveals it as it opens.
Item {
    id: count

    // Icons out of sight on this side.
    property int hidden: 0
    // The most there can be on a side, which sets the width.
    property int most: 0
    // Whether it's fading in or out right now.
    readonly property bool fading: fade.running

    width: widest.width
    height: label.implicitHeight

    Text {
        id: label
        anchors.fill: parent
        horizontalAlignment: Text.AlignHCenter
        // Fading out, it still shows the last one it had.
        text: `+${Math.max(1, count.hidden)}`
        color: Tokens.fgDim
        font.family: Tokens.fontMono
        font.pixelSize: Tokens.fontSizeMicro
        opacity: count.hidden > 0 ? 1 : 0

        Behavior on opacity {
            NumberAnimation {
                id: fade
                duration: Tokens.durationFast
            }
        }
    }

    TextMetrics {
        id: widest
        font: label.font
        text: `+${count.most}`
    }
}
