pragma ComponentBehavior: Bound

import QtQuick
import "../theme"

// The pop-ups, newest on top, each in a slot of its own, at most
// Tokens.notificationsShown at once; the rest wait, held, under a "+N more
// in the list" chip and come up as room frees. A new one grows out of the
// quick settings button above the stack's top right corner and, when its
// time runs out, folds back into it; one that's clicked, dismissed or
// closed elsewhere slides away. Several coming at once take their slots in
// one move, then open into them top to bottom, Tokens.notificationStagger
// apart. The same notification again (progress) updates its pop-up.
// No Quickshell here, so ui/tests can drive it; NotificationPopups puts it
// on a screen.
Item {
    id: stack

    property var iconSource: name => ""
    property real now: Date.now() / 1000

    // A pop-up was clicked, dismissed (✕), or one of its buttons clicked.
    signal opened(var notification)
    signal dismissed(int id)
    signal actionClicked(int id, string key)

    // Pop-ups shown, still leaving or waiting: anything left to draw.
    readonly property bool busy: shapes.length > 0 || waiting.length > 0
    // Where the cards are drawn, for the window's input and blur regions:
    // one plain item each, with no transforms, since a Quickshell Region
    // follows only its item's own position and size.
    property list<Item> shapes: []
    // Waiting entries, newest first.
    property var waiting: []

    // Shown entries by key: {key, notification, listed, leaving}.
    property var live: ({})
    property int nextKey: 1
    // Entries added since the stack last opened new ones, oldest first.
    property var arriving: []
    signal entryChanged(int key)
    // Entry `key` shows its notification as it is now; its time runs on.
    signal entryRefreshed(int key)
    // Entry `key` opens into its slot, after `delay` ms.
    signal entryOpens(int key, int delay)

    implicitWidth: Tokens.notificationWidth
    // The room every card takes once in place, counting those growing in
    // or leaving, so no card is ever drawn past it and the window keeps one
    // size through the motion rather than resizing a frame behind each step.
    implicitHeight: shapes.reduce((total, shape) => total + shape.room, 0) + (chip.visible ? chip.height : 0)

    // The shown entries, top to bottom (not those leaving).
    function shownEntries(): var {
        const entries = [];
        for (let i = 0; i < model.count; i++) {
            const entry = live[model.get(i).key];
            if (entry)
                entries.push(entry);
        }
        return entries;
    }

    // Takes in a notification to pop up.
    function add(notification: var) {
        const shown = shownEntries();
        const there = shown.findIndex(entry => entry.notification.id === notification.id);
        if (there >= 0) {
            shown[there].notification = notification;
            entryChanged(shown[there].key);
            return;
        }
        const held = waiting.findIndex(entry => entry.notification.id === notification.id);
        if (held >= 0) {
            waiting[held].notification = notification;
            waiting = [...waiting];
            return;
        }
        const entry = {
            key: nextKey++,
            notification: notification,
            listed: false,
            leaving: ""
        };
        live[entry.key] = entry;
        model.insert(0, {
            key: entry.key
        });
        // The oldest shown one waits for room again.
        if (shown.length + 1 > Tokens.notificationsShown) {
            const oldest = shown[shown.length - 1];
            take(oldest.key, "fold");
            waiting = [oldest, ...waiting];
        }
        arriving.push(entry.key);
        Qt.callLater(openArrivals);
    }

    // The ones that came together open, the newest (top) first.
    function openArrivals() {
        const keys = arriving.filter(key => live[key]).reverse();
        arriving = [];
        keys.forEach((key, at) => entryOpens(key, at * Tokens.notificationStagger));
    }

    // Takes pop-up `key` away: "fold" (back into the button) or "away".
    function remove(key: int, how: string) {
        if (!live[key])
            return;
        take(key, how);
        if (waiting.length > 0 && shownEntries().length < Tokens.notificationsShown) {
            const next = waiting[0];
            waiting = waiting.slice(1);
            next.leaving = "";
            next.key = nextKey++;
            live[next.key] = next;
            model.append({
                key: next.key
            });
            entryOpens(next.key, 0);
        }
    }

    // It leaves `live` now, and the model once it has gone (`gone`).
    function take(key: int, how: string) {
        live[key].leaving = how;
        entryChanged(key);
        delete live[key];
    }

    function gone(key: int) {
        for (let i = 0; i < model.count; i++) {
            if (model.get(i).key === key) {
                model.remove(i);
                return;
            }
        }
    }

    // The notifications open now (a notifications event's items and
    // transient ones): a pop-up shows its notification as it is there
    // (its app may have quit), and goes once it has left (dismissed in
    // quick settings, closed by its app).
    function sync(items: var) {
        const byId = new Map(items.map(item => [item.id, item]));
        // Waiting ones first, so none that has gone comes up in a freed place.
        for (const entry of waiting) {
            const now = byId.get(entry.notification.id);
            if (now) {
                entry.listed = true;
                entry.notification = now;
            }
        }
        waiting = waiting.filter(entry => !entry.listed || byId.has(entry.notification.id));
        for (const entry of shownEntries()) {
            const now = byId.get(entry.notification.id);
            if (now) {
                entry.listed = true;
                if (JSON.stringify(now) !== JSON.stringify(entry.notification)) {
                    entry.notification = now;
                    entryRefreshed(entry.key);
                }
            } else if (entry.listed) {
                remove(entry.key, "away");
            }
        }
    }

    // Every pop-up folds away (quick settings opened, with the list).
    function clear() {
        waiting = [];
        for (const entry of shownEntries())
            take(entry.key, "fold");
    }

    ListModel {
        id: model
    }

    Column {
        id: column
        width: Tokens.notificationWidth

        Repeater {
            model: model

            delegate: Item {
                id: slot

                required property int key
                property var entry: stack.live[key]
                // Its slot, 0 none to 1 its card's room: taken at once, so
                // cards coming together push the others down in one move,
                // and given back as its card leaves.
                property real room: 0
                // Its card, 0 gone to 1 in place: growing out of the button
                // (once it opens, `entryOpens`) and folding back, or sliding
                // away.
                property real presence: 0
                readonly property bool away: entry.leaving === "away"
                // Its card's transform: folded toward the button over its
                // top right corner and raised, or slid aside.
                readonly property real xScale: away ? 1 : Tokens.notificationFold + (1 - Tokens.notificationFold) * presence
                readonly property real yScale: away ? 1 : Tokens.notificationFoldHeight + (1 - Tokens.notificationFoldHeight) * presence
                readonly property real shiftX: away ? (1 - presence) * Tokens.notificationAway : 0
                readonly property real shiftY: away ? 0 : -(1 - presence) * Tokens.notificationRise

                // The card as drawn, in the stack.
                readonly property Item shape: Item {
                    parent: stack
                    x: slot.x + slot.width * (1 - slot.xScale) + slot.shiftX
                    y: slot.y + slot.shiftY
                    width: slot.width * slot.xScale
                    height: card.height * slot.yScale
                    readonly property real radius: card.radius * Math.min(slot.xScale, slot.yScale)
                    // Its slot's height once in place.
                    readonly property real room: card.implicitHeight + Tokens.gap
                }
                Component.onCompleted: stack.shapes = [...stack.shapes, shape]
                Component.onDestruction: stack.shapes = [...stack.shapes].filter(item => item !== shape)

                width: Tokens.notificationWidth
                height: (card.implicitHeight + Tokens.gap) * room
                z: -presence

                NumberAnimation on room {
                    id: making
                    to: 1
                    duration: Tokens.durationPanel
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Tokens.easing
                }

                SequentialAnimation {
                    id: motion
                    property alias delay: pause.duration
                    property alias to: grow.to

                    PauseAnimation {
                        id: pause
                    }
                    NumberAnimation {
                        id: grow
                        target: slot
                        property: "presence"
                        duration: Tokens.durationPanel
                        easing.type: Easing.BezierSpline
                        easing.bezierCurve: Tokens.easing
                    }
                    onFinished: {
                        if (slot.entry.leaving)
                            stack.gone(slot.key);
                    }
                }

                Connections {
                    target: stack
                    function onEntryOpens(opening: int, delay: int) {
                        if (opening !== slot.key || slot.entry.leaving)
                            return;
                        card.restart();
                        motion.delay = delay;
                        motion.to = 1;
                        motion.start();
                    }
                    function onEntryRefreshed(changed: int) {
                        if (changed === slot.key)
                            slot.entry = Object.assign({}, stack.live[slot.key]);
                    }
                    function onEntryChanged(changed: int) {
                        if (changed !== slot.key)
                            return;
                        // A copy, so the card sees the change; `take`
                        // drops it from `live` only after this.
                        slot.entry = Object.assign({}, stack.live[slot.key]);
                        if (slot.entry.leaving) {
                            making.stop();
                            making.to = 0;
                            making.start();
                            motion.stop();
                            motion.delay = 0;
                            motion.to = 0;
                            motion.start();
                        } else {
                            card.restart();
                        }
                    }
                }

                NotificationCard {
                    id: card
                    objectName: `card-${notification.id}`
                    width: parent.width
                    notification: slot.entry.notification
                    iconSource: stack.iconSource
                    now: stack.now
                    opacity: slot.presence
                    // Not before it opens, nor once it's leaving.
                    enabled: !slot.entry.leaving && slot.presence > 0
                    onExpired: stack.remove(slot.key, "fold")
                    onClicked: {
                        stack.opened(notification);
                        stack.remove(slot.key, "away");
                    }
                    onDismissed: {
                        stack.dismissed(notification.id);
                        stack.remove(slot.key, "away");
                    }
                    onActionClicked: key => {
                        stack.actionClicked(notification.id, key);
                        stack.remove(slot.key, "away");
                    }

                    transform: [
                        Scale {
                            origin.x: card.width
                            origin.y: 0
                            xScale: slot.xScale
                            yScale: slot.yScale
                        },
                        Translate {
                            x: slot.shiftX
                            y: slot.shiftY
                        }
                    ]
                }
            }
        }
    }

    Rectangle {
        id: chip
        visible: stack.waiting.length > 0
        anchors.horizontalCenter: column.horizontalCenter
        y: column.height
        width: chipText.implicitWidth + 2 * Tokens.pad
        height: Tokens.notificationActionHeight
        radius: height / 2
        color: Tokens.surface
        border.width: Tokens.dividerWidth
        border.color: Tokens.glassRim

        Text {
            id: chipText
            anchors.centerIn: parent
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeTiny
            color: Tokens.fgSoft
            text: qsTr("+%1 more in the list").arg(stack.waiting.length)
        }
    }
}
