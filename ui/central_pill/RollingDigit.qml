pragma ComponentBehavior: Bound

import QtQuick
import "../theme"

// One digit of the central pill's time. When it changes, the old digit rolls
// up and out while the new one rolls in from below; digits that stay the
// same don't move. As wide as the font's widest digit, so the time never
// shifts sideways.
Item {
    id: digit

    // The digit to show.
    required property string text
    property alias font: metrics.font
    property color color

    // What's shown now and what's rolling out.
    property string shown: text
    property string leaving: ""
    // 0 right after a change, 1 once the new digit is in place.
    property real roll: 1

    implicitWidth: metrics.advanceWidth
    implicitHeight: metrics.height
    clip: true

    onTextChanged: {
        leaving = shown;
        shown = text;
        rolling.restart();
    }

    TextMetrics {
        id: metrics
        // Tabular figures are all this wide.
        text: "0"
    }

    NumberAnimation {
        id: rolling
        target: digit
        property: "roll"
        from: 0
        to: 1
        duration: Tokens.durationPanel
        easing.type: Easing.BezierSpline
        easing.bezierCurve: Tokens.easing
    }

    component Glyph: Text {
        width: digit.width
        height: digit.height
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        font: metrics.font
        color: digit.color
        textFormat: Text.PlainText
    }

    Glyph {
        y: -digit.roll * digit.height
        opacity: 1 - digit.roll
        visible: digit.roll < 1
        text: digit.leaving
    }

    Glyph {
        y: (1 - digit.roll) * digit.height
        opacity: digit.roll
        text: digit.shown
    }
}
