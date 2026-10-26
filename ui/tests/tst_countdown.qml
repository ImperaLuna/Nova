import QtQuick
import QtTest
import "../theme"

// A card's time left: its pieces go out one per step, from the right, and
// it expires once; held, it stops mid-piece and goes on from there; with no
// time it never runs.
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

    function shown(countdown) {
        const row = countdown.children[0];
        return row.children.filter(piece => piece instanceof Rectangle && piece.visible);
    }

    function test_pieces_go_out_one_per_step_and_it_expires_once() {
        const started = Date.now();
        const countdown = make();
        compare(countdown.lit, Tokens.countdownSegments);
        compare(shown(countdown).length, Tokens.countdownSegments);
        const seen = [countdown.lit];
        countdown.litChanged.connect(() => seen.push(countdown.lit));
        tryCompare(expiries, "count", 1, 3000);
        verify(Date.now() - started >= countdown.duration * 0.9, "it took its time");
        compare(seen, [6, 5, 4, 3, 2, 1, 0], "one piece per step");
        compare(shown(countdown).length, 0);
        verify(!countdown.running);
        wait(200);
        compare(expiries.count, 1);
    }

    function test_held_it_stops_and_goes_on_from_there() {
        const countdown = make({
            duration: 1200
        });
        tryVerify(() => countdown.lit === Tokens.countdownSegments - 1, 1000);
        countdown.held = true;
        const before = countdown.lit;
        wait(500);
        compare(countdown.lit, before, "held");
        countdown.held = false;
        tryCompare(expiries, "count", 1, 3000);
    }

    function test_a_lit_piece_is_on_the_left() {
        const countdown = make();
        tryVerify(() => countdown.lit === Tokens.countdownSegments - 1, 1000);
        const pieces = shown(countdown);
        compare(pieces.length, Tokens.countdownSegments - 1);
        verify(pieces.every(piece => piece.x < countdown.width - piece.width), "the rightmost went first");
    }

    function test_restart_lights_them_all_again() {
        const countdown = make();
        tryVerify(() => countdown.lit < Tokens.countdownSegments - 1, 1000);
        countdown.restart();
        compare(countdown.lit, Tokens.countdownSegments);
        verify(countdown.running);
    }

    function test_with_no_time_it_never_runs() {
        const countdown = make({
            duration: 0
        });
        verify(!countdown.running);
        wait(200);
        compare(countdown.lit, Tokens.countdownSegments);
        compare(expiries.count, 0);
    }
}
