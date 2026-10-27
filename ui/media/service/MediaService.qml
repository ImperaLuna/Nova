pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol
import "../media.js" as Media

// The media players from novad, and the commands that act on them. The pill
// and its panel (../NowPlaying.qml) read and act through here, never through
// Daemon directly.
Singleton {
    id: root

    // The feature is on and novad has reported, so the pill can show.
    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_MEDIA) && Daemon.media !== null
    // Every player, in the order they appeared (novad's `media` event).
    readonly property var players: ready ? Daemon.media.players : []
    // The player the pill shows, or null when none is.
    readonly property var shown: Media.shownPlayer(ready ? Daemon.media : null)

    // Plays or pauses, or skips, on `player` (a player's id).
    function control(player: string, action: string) {
        Daemon.call(Protocol.CMD_MEDIA_CONTROL, {
            player: player,
            action: action
        }, reportError);
    }

    // Moves `player` to `position` microseconds into its track.
    function seek(player: string, position: real) {
        Daemon.call(Protocol.CMD_MEDIA_SEEK, {
            player: player,
            position_us: Math.round(position)
        }, reportError);
    }

    // Makes `player` the one the pill shows, until another starts playing.
    function show(player: string) {
        Daemon.call(Protocol.CMD_MEDIA_SHOW, {
            player: player
        }, reportError);
    }

    // Brings `player`'s app forward; `done(worked)` says whether it could.
    function focus(player: string, done: var) {
        Daemon.call(Protocol.CMD_MEDIA_FOCUS, {
            player: player
        }, (error, worked) => {
            reportError(error);
            if (done)
                done(!error && worked === true);
        });
    }

    // The workspace name where `player`'s app has a window, or "" when none
    // does; `done(name)` gets it.
    function workspace(player: string, done: var) {
        Daemon.call(Protocol.CMD_MEDIA_WINDOW, {
            player: player
        }, (error, name) => {
            reportError(error);
            if (done)
                done(error ? "" : (name ?? ""));
        });
    }

    function reportError(error) {
        if (error)
            console.warn(`nova: now playing: ${error}`);
    }
}
