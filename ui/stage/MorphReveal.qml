import QtQuick
import "../theme"

// Animates a 0..1 value of something inside a morphing panel, usually its
// opacity: `MorphReveal on opacity { delay: Tokens.revealContent }` with
// `opacity: morph.open ? 1 : 0`. Going to 1, it waits `delay` (a share of
// the panel's morph) and then runs for `duration`, so content appears once
// there's room for it. Going to 0, it follows the morph back, all at once.
Behavior {
    id: reveal

    // When it starts opening, as a share of the panel's morph.
    property real delay: 0
    property int duration: Tokens.durationFade
    // Fades run linearly; set for values that move, like a filling meter.
    property bool eased: false

    readonly property bool opening: targetValue === 1
    // Whether it's waiting or animating right now.
    readonly property bool running: steps.running

    SequentialAnimation {
        id: steps
        PauseAnimation {
            duration: reveal.opening ? Tokens.durationPanel * reveal.delay : 0
        }
        NumberAnimation {
            duration: reveal.opening ? reveal.duration : Tokens.durationPanel
            easing.type: reveal.opening && !reveal.eased ? Easing.Linear : Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }
}
