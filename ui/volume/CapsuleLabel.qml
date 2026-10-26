pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../util/numbers.js" as Numbers

// The icon and number at the bottom of the capsule, readable on glass and on
// liquid alike: drawn twice, in the glass's ink above the liquid's surface
// and the liquid's ink below it, each copy clipped at the surface.
Item {
    id: label

    // volume.js icon() and the number ("–" without an output).
    required property string glyph
    required property string text
    // Muted: the number fades back.
    property bool dimmed: false
    // The liquid's surface, from the top of this item.
    required property real surfaceY
    property color inkAbove: Tokens.fg
    property color inkBelow: Tokens.onAccent

    // The part of the label's box under the icon and number, for clicks.
    readonly property rect face: Qt.rect(above.content.x, above.content.y, above.content.width, above.content.height)

    component Copy: Item {
        id: copy
        required property color ink
        // The label's top, in this copy's coordinates.
        required property real labelTop
        readonly property Column content: column
        clip: true
        Column {
            id: column
            y: copy.labelTop + label.height - height - Tokens.capsuleLabelPad
            width: label.width
            spacing: Tokens.labelNudge
            VolumeIcon {
                anchors.horizontalCenter: parent.horizontalCenter
                glyph: label.glyph
                color: copy.ink
            }
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: label.text
                color: copy.ink
                opacity: label.dimmed ? Tokens.dimOpacity : 1
                font.family: Tokens.fontMono
                font.pixelSize: Tokens.fontSize
                font.weight: Font.DemiBold
                textFormat: Text.PlainText
            }
        }
    }

    Copy {
        id: above
        width: label.width
        height: Numbers.clamp(label.surfaceY, 0, label.height)
        ink: label.inkAbove
        labelTop: 0
    }
    Copy {
        id: below
        y: Numbers.clamp(label.surfaceY, 0, label.height)
        width: label.width
        height: label.height - y
        ink: label.inkBelow
        labelTop: -y
    }
}
