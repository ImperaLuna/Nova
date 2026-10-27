pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// The wallpaper carousel's link to novad: the images in the wallpaper
// folder, the ones shown now, and setting one. Also what the open carousel
// previews, so the other screens can show it too. The carousel
// (../WallpaperCarousel.qml) gets these through the stage and never talks
// to Daemon itself.
Singleton {
    id: root

    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_WALLPAPER)

    // While a carousel is open or closing: {paths, choosingFor, output,
    // shown}, the other screens' picks where they'd change (by name), the
    // screen it picks for ("" for every one), the screen it's on, and
    // whether it's open (the previews fade as it closes). The other screens
    // preview their picks from it. The stage sets it; null otherwise.
    property var preview: null

    // `done(error, folder, shown)`: the folder's {folder, images} and the
    // {path, outputs} shown now, or why novad couldn't say.
    function load(done: var) {
        Daemon.call(Protocol.CMD_WALLPAPER_FOLDER, undefined, (error, folder) => {
            if (error) {
                done(error, null, null);
                return;
            }
            Daemon.call(Protocol.CMD_WALLPAPERS, undefined, (shownError, shown) => {
                if (shownError)
                    console.warn(`nova: wallpapers: ${shownError}`);
                done(null, folder, shownError ? {} : shown);
            });
        });
    }

    // Shows `path` on every screen, or only on `output` when it's set;
    // `done(error)` gets novad's answer.
    function set(path: string, output: var, done: var) {
        const args = {
            path: path
        };
        if (output)
            args.output = output;
        Daemon.call(Protocol.CMD_SET_WALLPAPER, args, error => done(error ?? null));
    }

    // Makes each of `requests` ({path, output}) in turn, stopping at the
    // first that fails; `done(error)` gets the outcome.
    function setAll(requests: var, done: var) {
        if (requests.length === 0) {
            done(null);
            return;
        }
        const [first, ...rest] = requests;
        set(first.path, first.output, error => {
            if (error)
                done(error);
            else
                setAll(rest, done);
        });
    }
}
