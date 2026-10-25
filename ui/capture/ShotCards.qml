pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../compositor"
import "../stage"
import "service"

// Where saved screenshots and recordings land: the card, on the screen that
// had focus when it was saved. A new one while it's out replaces it in
// place.
Scope {
    id: cards

    property var shot: null
    // The shot is a recording.
    property bool video: false
    property ShellScreen screen: null
    // As the card lays out by it, kept while that screen goes away.
    readonly property StageScreen output: StageScreen {
        screen: cards.screen
    }

    Connections {
        target: ScreenshotService
        function onSaved(shot: var) {
            cards.show(shot, false);
        }
    }
    Connections {
        target: RecordingService
        function onSaved(saved: var) {
            cards.show(saved, true);
        }
    }

    function show(shot: var, video: bool) {
        if (!cards.shot || !cards.screen)
            cards.screen = Compositor.focusedScreen();
        cards.video = video;
        cards.shot = shot;
    }

    LazyLoader {
        active: cards.shot !== null && cards.screen !== null
        component: ShotCardWindow {
            output: cards.output
            shot: cards.shot
            video: cards.video
            onGone: cards.shot = null
        }
    }
}
