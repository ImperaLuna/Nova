pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Services.SystemTray
import "../../daemon"
import "../../daemon/protocol.js" as Protocol
import "../tray.js" as Tray
import "../../icons"

// The tray's link to its apps: Quickshell's SystemTray is the host that
// reads each item's icon and menu from the bus, and novad (the watcher)
// says which desktop app each one is and which ones the user hid. The bar
// and quick settings read the apps here and act through here.
Singleton {
    id: root

    // The feature is on, so novad watches for tray icons.
    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_TRAY)
    // Every app in the tray, in the order they came (tray.js `entries`).
    // Quickshell's items. Touched only while the feature is on: the first
    // touch starts its host. `var`, as qmllint can't see the model's values.
    readonly property var hosted: ready ? SystemTray.items : null
    readonly property var all: hosted ? Tray.entries(hosted.values, Daemon.tray) : []
    // What the bar shows: the apps not hidden, attention first.
    readonly property var shown: Tray.shown(all)
    readonly property bool attention: Tray.anyAttention(all)

    // Whether a click on the app's icon shows its menu: it has nothing
    // else to show. Otherwise the click activates it (`activate`), showing
    // or hiding its window.
    function opensMenu(entry: var): bool {
        return entry.item.onlyMenu && entry.item.hasMenu;
    }

    // A click on the app's icon: novad asks it to show its window, then
    // focuses that window, which the app can't do itself from a tray click
    // on niri. An item novad doesn't list is asked directly.
    function activate(entry: var) {
        if (!entry.address) {
            entry.item.activate();
            return;
        }
        Daemon.call(Protocol.CMD_ACTIVATE_TRAY_ITEM, {
            address: entry.address
        }, error => {
            if (error)
                console.warn(`nova: tray: ${error}`);
        });
    }

    function hide(key: string, hidden: bool) {
        Daemon.call(Protocol.CMD_HIDE_TRAY_APP, {
            id: key,
            hidden: hidden
        }, error => {
            if (error)
                console.warn(`nova: tray: ${error}`);
        });
    }

    // What an Image shows for an entry: the app's themed icon when novad
    // found its desktop entry, else the item's own image.
    function iconSource(entry: var): string {
        return AppIcons.source(entry.appIcon ?? "") || entry.icon;
    }
}
