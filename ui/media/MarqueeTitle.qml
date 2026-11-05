pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "media.js" as Media

// The pill's title, at most `maxWidth` wide. One too long fades out at its
// end; `scroll()` moves it through once (a pause at each end) and back to
// its start, where it rests. It never loops on its own: the pill scrolls it
// when the track changes, and it scrolls again and again while `hovered`.
// Only `running` and shown (its bar can be hidden beside a fullscreen
// window) scrolls it, so a hidden or open pill never animates.
Item {
    id: title

    required property string text
    property color color: Tokens.fg
    property int maxWidth: Tokens.mediaTitleMax
    // Whether it may scroll now; stopping puts it back at its start.
    property bool running: true
    // The pointer is on the pill: scroll through, and again once done.
    property bool hovered: false

    // Scrolling or changing color right now, for the stage's `moving`.
    readonly property bool moving: marquee.running || recolor.running
    // How much longer than its room the text is.
    readonly property real over: Math.max(0, label.implicitWidth - maxWidth)

    // Scrolls through once, unless it fits or it's scrolling already.
    function scroll() {
        if (running && visible && over > 0 && !marquee.running)
            marquee.start();
    }

    function rest() {
        marquee.stop();
        label.x = 0;
    }

    onRunningChanged: {
        if (!running)
            rest();
    }
    onTextChanged: rest()
    onVisibleChanged: {
        if (!visible)
            rest();
    }
    onHoveredChanged: {
        if (hovered)
            scroll();
    }

    implicitWidth: Math.min(label.implicitWidth, maxWidth)
    implicitHeight: label.implicitHeight
    clip: true

    // Only text that doesn't fit is drawn through the fade instead of as it
    // is: none at the start until the text has moved past it, none at the
    // end once its end is in view. Through its layer, not a
    // ShaderEffectSource of its own: one with no parent item took a window
    // reference it never gave back, and a removed screen left the title on
    // a deleted window (IMP-353).
    Item {
        id: strip
        width: title.width
        height: title.height
        layer.enabled: title.over > 0
        layer.smooth: true
        layer.effect: ShaderEffect {
            readonly property real fadeStart: Math.min(-label.x, Tokens.mediaFade)
            readonly property real fadeEnd: Math.min(title.over + label.x, Tokens.mediaFade)
            fragmentShader: Qt.resolvedUrl("fade.frag.qsb")
        }

        Text {
            id: label
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            font.weight: Tokens.weightTitle
            color: title.color
            textFormat: Text.PlainText
            text: title.text

            Behavior on color {
                ColorAnimation {
                    id: recolor
                    duration: Tokens.durationColor
                }
            }
        }
    }

    SequentialAnimation {
        id: marquee
        onFinished: {
            label.x = 0;
            // Not from inside its own finish: start it again once that's done.
            if (title.hovered)
                Qt.callLater(title.scroll);
        }
        PauseAnimation {
            duration: Tokens.durationMarqueeRest
        }
        NumberAnimation {
            target: label
            property: "x"
            from: 0
            to: -title.over
            duration: Media.marqueeDuration(title.over, Tokens.mediaMarqueeSpeed)
        }
        PauseAnimation {
            duration: Tokens.durationMarqueeRest
        }
    }
}
