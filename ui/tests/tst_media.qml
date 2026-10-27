import QtQuick
import QtTest
import "../media/media.js" as Media

// The now-playing pill's plain logic: which player is shown, the progress
// extrapolation and the times and labels the pill and panel read.
TestCase {
    name: "Media"

    readonly property var spotify: ({
            id: "org.mpris.MediaPlayer2.spotify",
            app: {
                id: "spotify.desktop",
                name: "Spotify"
            },
            identity: "Spotify",
            status: "playing",
            title: "Song",
            artist: "Artist",
            browser: false,
            length_us: 200000000,
            position_us: 1000000,
            position_at_ms: 1000,
            rate: 1.0
        })
    readonly property var firefox: ({
            id: "org.mpris.MediaPlayer2.firefox.instance_1",
            app: null,
            identity: "Firefox",
            status: "paused",
            title: "A video",
            artist: "Some channel",
            browser: true,
            length_us: 900000000,
            position_us: 0,
            position_at_ms: 0,
            rate: 1.0
        })

    function test_shows_the_named_player() {
        const media = {
            players: [spotify, firefox],
            shown: firefox.id
        };
        compare(Media.shownPlayer(media), firefox);
        compare(Media.shownPlayer({
            players: [spotify],
            shown: "gone"
        }), null);
        compare(Media.shownPlayer({
            players: [],
            shown: null
        }), null);
    }

    function test_playing_position_moves_on_at_its_rate() {
        // One second after its read, at normal speed: 1 s past the read position.
        compare(Media.positionAt(spotify, 2000), 2000000);
        const fast = Object.assign({}, spotify, {
            rate: 2.0
        });
        compare(Media.positionAt(fast, 2000), 3000000);
    }

    function test_paused_position_stays_put_and_stops_at_the_end() {
        compare(Media.positionAt(firefox, 99999), 0);
        const late = Object.assign({}, spotify, {
            position_us: 199000000
        });
        compare(Media.positionAt(late, 10000), 200000000);
    }

    function test_formats_times() {
        compare(Media.formatTime(0), "0:00");
        compare(Media.formatTime(65000000), "1:05");
        compare(Media.formatTime(3723000000), "1:02:03");
        compare(Media.formatTime(-5), "0:00");
    }

    function test_fraction_is_clamped() {
        compare(Media.fraction(50, 200), 0.25);
        compare(Media.fraction(300, 200), 1);
        compare(Media.fraction(10, 0), 0);
    }

    function test_browsers_show_no_artist_in_the_pill() {
        compare(Media.pillArtist(spotify), "Artist");
        compare(Media.pillArtist(firefox), "");
    }

    function test_go_to_row_names_the_workspace_when_known() {
        compare(Media.goToLabel(spotify, 3), "Go to Spotify · workspace 3");
        compare(Media.goToLabel(spotify, ""), "Go to Spotify");
        compare(Media.goToLabel(firefox, ""), "Go to Firefox");
    }
}
