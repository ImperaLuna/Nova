pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Effects

// The soft shadow under a piece of glass, cut out where the glass is: the
// glass is see-through, and a shadow under it would only darken it. Put it
// right before its `glass`, in the same parent: it follows its size, place
// and radius.
Item {
    id: shadow

    required property Rectangle glass

    x: glass.x - Tokens.shadowRoom
    y: glass.y - Tokens.shadowRoom
    width: glass.width + 2 * Tokens.shadowRoom
    height: glass.height + 2 * Tokens.shadowRoom
    layer.enabled: true
    layer.effect: MultiEffect {
        maskEnabled: true
        maskInverted: true
        maskSource: cutout
    }

    RectangularShadow {
        x: Tokens.shadowRoom
        y: Tokens.shadowRoom
        width: shadow.glass.width
        height: shadow.glass.height
        radius: shadow.glass.radius
        offset.y: Tokens.shadowOffset
        blur: Tokens.shadowBlur
        color: Tokens.shadow
    }

    Item {
        id: cutout
        width: shadow.width
        height: shadow.height
        visible: false
        layer.enabled: true

        Rectangle {
            x: Tokens.shadowRoom
            y: Tokens.shadowRoom
            width: shadow.glass.width
            height: shadow.glass.height
            radius: shadow.glass.radius
        }
    }
}
