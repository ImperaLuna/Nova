import QtQuick
import QtTest
import "../theme"

// A card's time left: one line draining toward the left on a fixed tick,
// expiring once; held, it stops where it is and goes on from there; with
// no time it never runs.
TestCase {
    name: "Countdown"

    Component {
        id: countdownComponent
        Countdown {
            width: 200
            height: 2
            duration: 600
        }
    }

    SignalSpy {
        id: expiries
        signalName: "expired"
    }

    function make(properties) {
        const countdown = createTemporaryObject(countdownComponent, null, properties ?? {});
        expiries.clear();
        expiries.target = countdown;
        return countdown;
    }

    function line(countdown) {
        return findChild(countdown, "line");
    }

    function test_it_drains_on_its_tick_and_expires_once() {
        const started = Date.now();
        const countdown = make();
        compare(countdown.remaining, 1);
        compare(line(countdown).width, countdown.width);
        const seen = [countdown.remaining];
        countdown.remainingChanged.connect(() => seen.push(countdown.remaining));
        tryCompare(expiries, "count", 1, 3000);
        verify(Date.now() - started >= countdown.duration * 0.9, "it took its time");
        const steps = countdown.duration / Tokens.countdownTick;
        verify(seen.length > 2 && seen.length <= steps + 2, `at most one step per tick: ${seen.length}`);
        verify(seen.every((value, at) => at === 0 || value < seen[at - 1]), "it only drains");
        compare(countdown.remaining, 0);
        verify(!line(countdown).visible);
        verify(!countdown.running);
        wait(200);
        compare(expiries.count, 1);
    }

    function test_held_it_stops_and_goes_on_from_there() {
        const countdown = make({
            duration: 1200
        });
        tryVerify(() => countdown.remaining < 0.8, 1000);
        countdown.held = true;
        const before = countdown.remaining;
        wait(500);
        compare(countdown.remaining, before, "held");
        countdown.held = false;
        tryCompare(expiries, "count", 1, 3000);
    }

    function test_the_line_shrinks_from_the_right() {
        const countdown = make();
        tryVerify(() => countdown.remaining < 0.6, 1000);
        const drawn = line(countdown);
        compare(drawn.x, 0);
        fuzzyCompare(drawn.width, countdown.width * countdown.remaining, 0.01);
    }

    function test_restart_fills_it_again() {
        const countdown = make();
        tryVerify(() => countdown.remaining < 0.8, 1000);
        countdown.restart();
        compare(countdown.remaining, 1);
        verify(countdown.running);
    }

    function test_with_no_time_it_never_runs() {
        const countdown = make({
            duration: 0
        });
        verify(!countdown.running);
        wait(200);
        compare(countdown.remaining, 1);
        compare(expiries.count, 0);
    }
}
