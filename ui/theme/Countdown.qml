pragma ComponentBehavior: Bound

import QtQuick

// A card's time left, as one line draining toward the left. It moves on a
// fixed tick (Tokens.countdownTick), not every vblank, so it looks smooth
// but costs a few frames a second. `held` (the pointer on the card) stops
// it where it is. `expired` comes when it runs out; a duration of 0 or less
// never runs.
Item {
    id: countdown

    property int duration
    property bool held: false
    property color color: Tokens.notificationTimer
    property real radius: height / 2
    // The share of its time left, 1 to 0.
    property real remaining: 1
    readonly property bool running: tick.running

    signal expired

    // Time counted before the current run, and when that run started.
    property real spent: 0
    property real since: 0

    // Starts over, full.
    function restart() {
        spent = 0;
        remaining = 1;
        resume();
    }

    function resume() {
        tick.stop();
        since = 0;
        if (duration <= 0 || held || remaining <= 0)
            return;
        since = Date.now();
        tick.start();
    }

    onHeldChanged: {
        if (!held)
            return resume();
        // Kept within its time, should the clock jump meanwhile.
        if (since > 0)
            spent = Math.min(Math.max(spent + Date.now() - since, spent), duration);
        since = 0;
        tick.stop();
    }
    Component.onCompleted: {
        restart();
        durationChanged.connect(restart);
    }

    Timer {
        id: tick
        interval: Tokens.countdownTick
        repeat: true
        onTriggered: {
            const elapsed = Math.max(0, countdown.spent + Date.now() - countdown.since);
            countdown.remaining = Math.max(0, 1 - elapsed / countdown.duration);
            if (countdown.remaining > 0)
                return;
            stop();
            countdown.expired();
        }
    }

    Rectangle {
        objectName: "line"
        width: countdown.width * countdown.remaining
        height: countdown.height
        radius: countdown.radius
        color: countdown.color
        visible: countdown.remaining > 0
    }
}
