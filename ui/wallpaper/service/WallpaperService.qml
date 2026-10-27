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

    // While a carousel is open or closing: {path, output, everywhere, shown},
    // the image it previews, the screen it's on, whether a pick changes
    // every screen, and whether it's open (the preview fades as it closes).
    // The other screens preview it too while `everywhere`. The stage sets
    // it; null otherwise.
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
}
