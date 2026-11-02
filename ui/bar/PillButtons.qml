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
    // The same as a model that changes only where they do.
    ListModel {
        id: shown
    }
    onSlotsChanged: Slots.sync(shown, slots)
    Component.onCompleted: Slots.sync(shown, slots)

    spacing: Tokens.pillButtonGap

    Repeater {
        model: shown

        Loader {
            // Its view's id.
            required property string view

            Component.onCompleted: {
                const url = Views.url(view);
                if (url.toString() === "") {
                    console.warn(`nova: no view ${view} for ${Slots.HOST_RIGHT_PILL_BUTTONS}`);
                    return;
                }
                setSource(url, {
                    stage: buttons.stage
                });
            }
        }
    }
}
