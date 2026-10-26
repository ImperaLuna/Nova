import QtQuick
import QtTest
import "../text/duration.js" as Duration
import "../text/files.js" as Files

// How Nova writes a span of time (an uptime, a countdown, how long ago, a
// recording's length) and a file's URL.
TestCase {
    name: "Text"

    function test_minutes_read_as_the_two_biggest_units() {
        compare(Duration.minutes(0), "0m");
        compare(Duration.minutes(45), "45m");
        compare(Duration.minutes(60), "1h");
        compare(Duration.minutes(3 * 60 + 12), "3h 12m");
        compare(Duration.minutes(24 * 60 + 59), "1d", "minutes drop out after a day");
        compare(Duration.minutes((3 * 24 + 2) * 60 + 5), "3d 2h");
    }

    function test_uptime_reads_like_other_spans() {
        compare(Duration.uptime(59), "up 0m");
        compare(Duration.uptime(12 * 60), "up 12m");
        compare(Duration.uptime((3 * 60 + 12) * 60), "up 3h 12m");
        compare(Duration.uptime((50 * 60 + 5) * 60), "up 2d 2h");
        compare(Duration.uptime(-5), "up 0m", "a clock that went back");
    }

    function test_how_long_ago_reads_in_its_biggest_unit() {
        compare(Duration.ago(1000, 1030), "now");
        compare(Duration.ago(1000, 1000 + 5 * 60), "5m");
        compare(Duration.ago(1000, 1000 + 2 * 3600 + 10), "2h");
        compare(Duration.ago(1000, 1000 + 3 * 86400), "3d");
        compare(Duration.ago(1000, 900), "now", "a clock that went back");
    }

    function test_a_running_time_reads_like_a_stopwatch() {
        compare(Duration.clock(0), "0:00");
        compare(Duration.clock(42999), "0:42");
        compare(Duration.clock(3723000), "1:02:03");
        compare(Duration.twoDigits(5), "05");
        compare(Duration.twoDigits(42), "42");
    }

    function test_paths_become_escaped_file_urls() {
        compare(Files.fileUrl("/opt/My App/icon#1.png"), "file:///opt/My%20App/icon%231.png");
        compare(Files.fileUrl("/home/me/shots#1/a b?.png"), "file:///home/me/shots%231/a%20b%3F.png");
    }
}
