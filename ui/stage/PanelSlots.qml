pragma ComponentBehavior: Bound

import QtQml
import QtQuick
import Quickshell
import "../daemon"
import "../slots/slots.js" as Slots
import "../slots"

// The stage.panels host: the panels on the screen that aren't a pill of
// their own (the launcher, quick settings, the power menu, the wallpaper
// carousel), from the ready features' `[[slot]]` entries there, in their
// `order`. Each view (ui/slots/Views.qml) is created with the screen's
// stage and hands it its `panels` (StagePanel), which load only while
// their service is ready.
Scope {
    id: host

    // The screen's stage (Stage.qml).
    required property var stage

    // Its slots, in order.
    readonly property var wanted: Slots.forHost(Daemon.features, Slots.HOST_STAGE_PANELS)
    // The same as a model that changes only where they do, so the panels
    // that stay (and their state) are kept.
    ListModel {
        id: shown
    }
    onWantedChanged: Slots.sync(shown, wanted)
    Component.onCompleted: Slots.sync(shown, wanted)

    // The views created so far, by view id.
    property var created: ({})
    // Their panels, in their slots' order.
    readonly property list<StagePanel> panels: wanted.map(slot => created[slot.view]).filter(view => view).reduce((all, view) => [...all, ...view.panels], [])

    Instantiator {
        model: shown

        QtObject {
            id: entry

            // Its view's id.
            required property string view
            property QtObject created: null

            Component.onCompleted: {
                const url = Views.url(view);
                const component = url.toString() !== "" ? Qt.createComponent(url) : null;
                if (!component || component.status !== Component.Ready) {
                    console.warn(`nova: no view ${view} for ${Slots.HOST_STAGE_PANELS}${component ? `: ${component.errorString()}` : ""}`);
                    return;
                }
                created = component.createObject(host, {
                    stage: host.stage
                });
                host.created = Object.assign({}, host.created, {
                    [view]: created
                });
            }
            Component.onDestruction: {
                created?.destroy();
                // Unless a newer one has taken its place.
                if (host.created[view] !== created)
                    return;
                const left = Object.assign({}, host.created);
                delete left[view];
                host.created = left;
            }
        }
    }
}
