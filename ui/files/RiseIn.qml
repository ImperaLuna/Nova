import QtQuick
import "../theme"

// How an entry of Nova Files' file list arrives when a folder's entries
// have just come in: hidden and a little low, it waits its turn (`order`
// entries from where the list opened, each a stagger apart), then fades
// in as it eases up into place through `lift`, the entry's Translate.
// It starts by itself as it's made, unless `order` is -1 (no arrival).
SequentialAnimation {
    id: rise

    required property Item item
    required property Translate lift
    required property int order

    Component.onCompleted: {
        if (order >= 0)
            start();
    }

    PropertyAction {
        target: rise.item
        property: "opacity"
        value: 0
    }
    PropertyAction {
        target: rise.lift
        property: "y"
        value: Tokens.filesRowRise
    }
    PauseAnimation {
        duration: Math.max(rise.order, 0) * Tokens.filesRowStagger
    }
    ParallelAnimation {
        NumberAnimation {
            target: rise.item
            property: "opacity"
            to: 1
            duration: Tokens.durationRowIn
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
        NumberAnimation {
            target: rise.lift
            property: "y"
            to: 0
            duration: Tokens.durationRowIn
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }
}
