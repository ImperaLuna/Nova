pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../daemon"
import "../slots/slots.js" as Slots
import "../slots/views.js" as Views

// The shell.overlays host: what draws in windows of its own over the
// screens rather than on a stage (the volume capsule, the lock screen,
// notification pop-ups, the capture overlay, Bluetooth pairing questions),
// from the ready features' `[[slot]]` entries there. The shell draws it
// whatever else is on, the bar too.
//
// A view is created once and kept while its slot stays, so a features
// event that changes something else never rebuilds it (a lock screen torn
// down mid-lock would leave the screens covered). A view listed by more
// than one feature (the capture overlay, for screenshots and recordings)
// shows once. A view whose `held` is true stays after its feature turns
// off, until `held` goes false: the lock screen while the session is
// locked.
Scope {
    id: host

    // Nothing is drawn until novad has sent the theme's colors.
    readonly property var wanted: Daemon.theme ? Slots.forHost(Daemon.features, Slots.HOST_SHELL_OVERLAYS).map(slot => slot.view) : []
    onWantedChanged: sync()
    Component.onCompleted: sync()

    // The views shown, by view id.
    property var views: ({})

    // Creates the wanted views missing, and destroys those no longer wanted
    // and not held.
    function sync(): void {
        const shown = Object.assign({}, views);
        for (const view of wanted) {
            if (!shown[view])
                shown[view] = create(view);
        }
        for (const view of Object.keys(shown)) {
            if (!wanted.includes(view) && shown[view]?.held !== true) {
                shown[view]?.destroy();
                delete shown[view];
            }
        }
        views = shown;
    }

    function create(view: string): QtObject {
        const file = Views.path(view);
        const component = file !== "" ? Qt.createComponent(Qt.resolvedUrl(`../${file}`)) : null;
        if (!component || component.status !== Component.Ready) {
            console.warn(`nova: no view ${view} for ${Slots.HOST_SHELL_OVERLAYS}${component ? `: ${component.errorString()}` : ""}`);
            return null;
        }
        const created = component.createObject(host);
        // A held view that's let go may have to go now.
        if (created && created.heldChanged)
            created.heldChanged.connect(host.sync);
        return created;
    }
}
