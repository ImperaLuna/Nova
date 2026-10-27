pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Effects
import "../theme"

// The now-playing pill's title: one line in a slot the width of the title or
// of the pill's title room, whichever is less. A title wider than the slot is
// clipped and faded at its end. `scroll` moves the text inside the slot, for
// the marquee; the slot itself never moves.
Item {
    id: root

    property string text: ""
    property color color: Tokens.fg
    // How far the text is moved left inside the slot, in pixels, 0 at rest.
    property real scroll: 0
    // How much wider than the slot the text is; 0 when it fits.
    readonly property real overflow: Math.max(0, metrics.advanceWidth - Tokens.mediaTitleWidth)

    width: Math.min(metrics.advanceWidth, Tokens.mediaTitleWidth)
    height: Tokens.barHeight
    clip: true

    TextMetrics {
        id: metrics
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSize
        font.weight: Font.DemiBold
        text: root.text
    }

    // The text, faded at its end when it overflows: masked as a layer.
    Item {
        id: content
        width: root.width
        height: root.height
        layer.enabled: root.overflow > 0
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: fadeMaskItem
        }

        // The fade at the slot's end: a mask over the text.
        Item {
            id: fadeMaskItem
            layer.enabled: true
            visible: false
            width: Tokens.mediaTitleWidth
            height: Tokens.barHeight
            Rectangle {
                anchors.fill: parent
                gradient: Gradient {
                    orientation: Gradient.Horizontal
                    GradientStop {
                        position: 0
                        color: "white"
                    }
                    GradientStop {
                        position: 1 - Tokens.mediaFadeWidth / Tokens.mediaTitleWidth
                        color: "white"
                    }
                    GradientStop {
                        position: 1
                        color: "transparent"
                    }
                }
            }
        }

        Text {
            id: label
            x: root.scroll
            y: (content.height - height) / 2
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            font.weight: Font.DemiBold
            color: root.color
            textFormat: Text.PlainText
            text: root.text
        }
    }
}
