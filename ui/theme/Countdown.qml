pragma ComponentBehavior: Bound

import QtQuick

// A card's time left, as a row of `segments` short pieces: one goes out,
// from the right, each `duration` / `segments`, with no animation, so the
// whole countdown costs `segments` frames rather than one every vblank.
// `held` (the pointer on the card) holds it mid-piece. `expired` comes when
// the last piece goes; a duration of 0 or less never runs.
Item {
    id: countdown

    property int duration
    property bool held: false
    readonly property int segments: Tokens.countdownSegments
    property color color: Tokens.notificationTimer
    property real radius: height / 2
    // Pieces still lit, `segments` to 0.
    property int lit: segments
    readonly property bool running: tick.running

    signal expired

    readonly property real step: duration / segments
    // Time counted before the current run, and when that run started.
    property real spent: 0
    property real since: 0

    // Starts over with every piece lit.
    function restart() {
        spent = 0;
        lit = segments;
        resume();
    }

    function resume() {
        tick.stop();
        since = 0;
        if (duration <= 0 || held || lit <= 0)
            return;
        since = Date.now();
        tick.interval = Math.max(1, (segments - lit + 1) * step - spent);
        tick.start();
    }

    onHeldChanged: {
        if (!held)
            return resume();
        // Kept inside the current piece, should the clock jump meanwhile.
        if (since > 0)
            spent = Math.min(Math.max(spent + Date.now() - since, (segments - lit) * step), (segments - lit + 1) * step);
        since = 0;
        tick.stop();
    }
    Component.onCompleted: {
        restart();
        durationChanged.connect(restart);
    }

    Timer {
        id: tick
        onTriggered: {
            countdown.lit -= 1;
            // Snapped to the piece's end, so a timer that fires a little
            // early or late doesn't add up over the pieces.
            countdown.spent = (countdown.segments - countdown.lit) * countdown.step;
            if (countdown.lit > 0)
                countdown.resume();
            else
                countdown.expired();
        }
    }

    Row {
        anchors.fill: parent
        spacing: Tokens.countdownGap

        Repeater {
            model: countdown.segments

            Rectangle {
                required property int index
                width: (countdown.width - (countdown.segments - 1) * Tokens.countdownGap) / countdown.segments
                height: countdown.height
                radius: countdown.radius
                color: countdown.color
                visible: index < countdown.lit
            }
        }
    }
}
