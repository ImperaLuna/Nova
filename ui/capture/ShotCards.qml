pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../compositor"
import "service"

// Where saved screenshots land: the card, on the screen that had focus when
// the shot was saved. A new shot while it's out replaces it in place.
Scope {
    id: cards

    property var shot: null
    property ShellScreen screen: null

    Connections {
        target: ScreenshotService
        function onSaved(shot: var) {
            if (!cards.shot)
                cards.screen = Compositor.focusedScreen();
            cards.shot = shot;
        }
    }

    LazyLoader {
        active: cards.shot !== null && cards.screen !== null
        component: ShotCardWindow {
            output: cards.screen
            shot: cards.shot
            onGone: cards.shot = null
        }
    }
}
