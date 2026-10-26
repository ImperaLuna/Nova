pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../compositor"
import "../stage"
import "../theme"
import "service"

// The pop-ups' own layer surface, under the right pill of the screen that
// had focus when the first of them came; they stay there until they've all
// gone. On the overlay layer like the screenshot card, only as big as the
// stack plus its shadow, taking no keyboard and no space, and clicks and
// blur only on the cards, so they show over fullscreen windows too. None
// while quick settings shows the list: the list has them.
PanelWindow { // qmllint disable uncreatable-type
    id: window

    // Where the pop-ups are, or the next ones come out; kept while that
    // screen goes away.
    property ShellScreen target: null
    readonly property StageScreen output: StageScreen {
        screen: window.target
    }

    screen: target
    visible: stack.busy
    anchors.top: true
    anchors.right: true
    // The stack sits this far down, under the bar; the room above it is
    // where the pop-ups grow out of the quick settings button.
    readonly property real top: Tokens.barGap + Tokens.barHeight + Tokens.gap
    implicitWidth: Tokens.notificationWidth + Tokens.barMargin + Tokens.shadowRoom
    implicitHeight: Math.ceil(top + stack.implicitHeight + Tokens.shadowRoom)
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "nova-notifications"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    function popUp(notification: var) {
        if (NotificationsService.listOpen)
            return;
        if (!stack.busy)
            target = Compositor.focusedScreen();
        console.info(`nova: notification ${notification.id} pops up on ${output.name}`);
        stack.add(notification);
        // The list came first: a pop-up already in it is marked listed now,
        // so it goes when the list next loses it.
        stack.sync(NotificationsService.popUps);
    }

    Connections {
        target: NotificationsService
        function onPopped(notification: var) {
            window.popUp(notification);
        }
        function onPopUpsChanged() {
            stack.sync(NotificationsService.popUps);
        }
        function onListOpenChanged() {
            if (NotificationsService.listOpen)
                stack.clear();
        }
    }

    mask: Region {
        regions: inputRegions.instances
    }
    Variants {
        id: inputRegions
        model: stack.cards
        ShapeRegion {}
    }
    Region {
        id: blur
        regions: blurRegions.instances
    }
    Variants {
        id: blurRegions
        model: stack.cards
        ShapeRegion {}
    }
    Component.onCompleted: BackgroundEffect.blurRegion = blur

    // The minute shown as "5m", while pop-ups are up.
    property real now: Date.now() / 1000
    Timer {
        interval: 60 * 1000
        running: stack.busy
        repeat: true
        onTriggered: window.now = Date.now() / 1000
    }

    NotificationStack {
        id: stack
        x: Tokens.shadowRoom
        y: window.top
        iconSource: name => NotificationsService.iconSource(name)
        now: window.now
        onOpened: notification => NotificationsService.open(notification, window.output.name)
        onDismissed: id => NotificationsService.dismiss(id)
        onActionClicked: (id, key) => NotificationsService.act(id, key)
    }
}
