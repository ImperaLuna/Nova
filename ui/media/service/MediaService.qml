pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// The media players from novad, and the commands that control them. The
// now-playing pill and its panel (../NowPlaying.qml) read and act through
// here, never through Daemon directly.
Singleton {
    id: root

    // The latest media event's data ({players, shown}), or null.
    readonly property var media: Daemon.media
    // The feature is on and novad has reported: the pill can load.
    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_MEDIA) && media !== null

    // Plays or pauses `player`, or skips a track (a Protocol.MEDIA_* action).
    function control(player: string, action: string) {
        Daemon.call(Protocol.CMD_MEDIA_CONTROL, {
            player: player,
            action: action
        });
    }

    // Moves `player` to `positionUs` into its track.
    function seek(player: string, positionUs: real) {
        Daemon.call(Protocol.CMD_MEDIA_SEEK, {
            player: player,
            position_us: Math.round(positionUs)
        });
    }

    // Shows `player` in the bar instead of the one novad picked.
    function show(player: string) {
        Daemon.call(Protocol.CMD_MEDIA_SHOW, {
            player: player
        });
    }

    // Brings `player`'s window forward.
    function focus(player: string) {
        Daemon.call(Protocol.CMD_MEDIA_FOCUS, {
            player: player
        });
    }

    // Asks which workspace `player`'s window is on: `found(name)` gets its
    // name, or "" when novad can't tell.
    function window(player: string, found: var) {
        Daemon.call(Protocol.CMD_MEDIA_WINDOW, {
            player: player
        }, (error, workspace) => {
            if (error)
                console.warn(`nova: media: ${error}`);
            found(error ? "" : workspace ?? "");
        });
    }
}
