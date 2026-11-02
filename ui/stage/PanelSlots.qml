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

    // The slots shown, kept while a features event changes nothing here so
    // the panels stay.
    property var shown: []
    readonly property var wanted: Slots.forHost(Daemon.features, Slots.HOST_STAGE_PANELS)
    onWantedChanged: {
        if (!Slots.same(shown, wanted))
            shown = wanted;
    }
    Component.onCompleted: shown = wanted

    // The views created so far, by view id.
    property var created: ({})
    // Their panels, in their slots' order.
    readonly property list<StagePanel> panels: shown.map(slot => created[slot.view]).filter(view => view).reduce((all, view) => [...all, ...view.panels], [])

    Instantiator {
        model: host.shown

        QtObject {
            id: entry

            required property var modelData
            property QtObject view: null

            Component.onCompleted: {
                const url = Views.url(modelData.view);
                const component = url.toString() !== "" ? Qt.createComponent(url) : null;
                if (!component || component.status !== Component.Ready) {
                    console.warn(`nova: no view ${modelData.view} for ${Slots.HOST_STAGE_PANELS}${component ? `: ${component.errorString()}` : ""}`);
                    return;
                }
                view = component.createObject(host, {
                    stage: host.stage
                });
                host.created = Object.assign({}, host.created, {
                    [modelData.view]: view
                });
            }
            Component.onDestruction: {
                const left = Object.assign({}, host.created);
                delete left[modelData.view];
                host.created = left;
                view?.destroy();
            }
        }
    }
}
