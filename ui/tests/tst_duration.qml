import QtQuick
import QtTest
import "../text/duration.js" as Duration

// How Nova writes a span of time (an uptime, a countdown).
TestCase {
    name: "Duration"

    function test_minutes_read_as_the_two_biggest_units() {
        compare(Duration.minutes(0), "0m");
        compare(Duration.minutes(45), "45m");
        compare(Duration.minutes(60), "1h");
        compare(Duration.minutes(3 * 60 + 12), "3h 12m");
        compare(Duration.minutes(24 * 60 + 59), "1d", "minutes drop out after a day");
        compare(Duration.minutes((3 * 24 + 2) * 60 + 5), "3d 2h");
    }
}
