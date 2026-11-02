pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../slots/slots.js" as Slots
import "../slots"

// The right pill's buttons, the right-pill.buttons host: the views of
// `slots`, the ready features' `[[slot]]` entries there (quick settings'
// button, the orange power button), left to right by their `order`. On the pill, and
// in the same place on quick settings, which grows out of the pill over
// it. Each view is loaded with the screen's stage (Stage.qml), which it
// opens its panel through, and is a PillButton or contains one.
Row {
    id: buttons

    // The screen's stage.
    required property var stage
    // Slots.forHost(Daemon.features, Slots.HOST_RIGHT_PILL_BUTTONS).
    required property var slots
    // Kept while a features event changes nothing here.
    property var shown: []
    onSlotsChanged: {
        if (!Slots.same(shown, slots))
            shown = slots;
    }
    Component.onCompleted: shown = slots

    spacing: Tokens.pillButtonGap

    Repeater {
        model: buttons.shown

        Loader {
            required property var modelData

            Component.onCompleted: {
                const url = Views.url(modelData.view);
                if (url.toString() === "") {
                    console.warn(`nova: no view ${modelData.view} for ${Slots.HOST_RIGHT_PILL_BUTTONS}`);
                    return;
                }
                setSource(url, {
                    stage: buttons.stage
                });
            }
        }
    }
}
