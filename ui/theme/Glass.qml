import QtQuick

// Nova's smoked glass. The compositor blurs whatever is behind it (the
// window's BackgroundEffect region); this paints on top of that blur: a warm
// near-black tint that darkens what's behind without greying it, then a dark
// outline outside, a faint light line inside it, and a highlight along the
// top that fades out before the corners. Children go on top.
Rectangle {
    id: glass

    // Where its edge stops on the right, for glass whose outline runs on into
    // something drawn beside it (the power tab into its fillets).
    property real rimEnd: width

    color: Tokens.glass

    Item {
        width: glass.rimEnd
        height: glass.height
        clip: glass.rimEnd < glass.width

        // The outline, then the light line just inside it.
        Rectangle {
            width: glass.width
            height: glass.height
            radius: glass.radius
            color: "transparent"
            border.width: Tokens.glassEdgeWidth
            border.color: Tokens.glassOutline
        }
        Rectangle {
            x: Tokens.glassEdgeWidth
            y: Tokens.glassEdgeWidth
            width: glass.width - 2 * Tokens.glassEdgeWidth
            height: glass.height - 2 * Tokens.glassEdgeWidth
            radius: Math.max(0, glass.radius - Tokens.glassEdgeWidth)
            color: "transparent"
            border.width: Tokens.glassEdgeWidth
            border.color: Tokens.glassInnerLine
        }
    }

    // The highlight along the top, between the corners' curves.
    Rectangle {
        x: glass.radius * Tokens.glassHighlightInset
        y: Tokens.glassEdgeWidth
        width: Math.max(0, glass.rimEnd - 2 * x)
        height: Tokens.glassEdgeWidth
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop {
                position: 0
                color: "transparent"
            }
            GradientStop {
                position: Tokens.glassHighlightFade
                color: Tokens.glassHighlight
            }
            GradientStop {
                position: 1 - Tokens.glassHighlightFade
                color: Tokens.glassHighlight
            }
            GradientStop {
                position: 1
                color: "transparent"
            }
        }
    }
}
