pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "notifications.js" as Notifications

// The pop-ups, newest on top, at most Tokens.notificationsShown at once;
// the rest wait, held, under a "+N more in the list" chip and come up as
// room frees. A new one grows out of the quick settings button above the
// stack's top right corner and, when its time runs out, folds back into
// it; one that's clicked, dismissed or closed elsewhere slides away. A
// newer one from the same app takes a plain pop-up's place and counts
// ("+1"); the same notification again (progress) updates its pop-up.
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
    readonly property bool busy: cards.length > 0 || waiting.length > 0
    // The cards shown, for the window's input and blur regions.
    property list<Item> cards: []
    // Waiting entries, newest first.
    property var waiting: []

    // Shown entries by key: {key, notification, count, listed, leaving}.
    property var live: ({})
    property int nextKey: 1
    signal entryChanged(int key)
    // Entry `key` shows its notification as it is now; its time runs on.
    signal entryRefreshed(int key)

    implicitWidth: Tokens.notificationWidth
    implicitHeight: column.height + (chip.visible ? chip.height : 0)

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

    function asPlaces(entries: var): var {
        return entries.map(entry => ({
                    id: entry.notification.id,
                    app: entry.notification.app,
                    mergeable: Notifications.merges(entry.notification)
                }));
    }

    // Takes in a notification to pop up.
    function add(notification: var) {
        const shown = shownEntries();
        const there = Notifications.placeIn(asPlaces(shown), notification);
        if (there) {
            update(shown[there.at], notification, there.merged);
            entryChanged(shown[there.at].key);
            return;
        }
        const held = Notifications.placeIn(asPlaces(waiting), notification);
        if (held) {
            update(waiting[held.at], notification, held.merged);
            waiting = [...waiting];
            return;
        }
        const entry = {
            key: nextKey++,
            notification: notification,
            count: 1,
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
    }

    function update(entry: var, notification: var, merged: bool) {
        entry.notification = notification;
        if (merged)
            entry.count++;
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
                // 0 gone, 1 in place: growing out of the button and folding
                // back, or sliding away. Its room grows and shrinks with
                // it, so the others move along.
                property real presence: 0
                readonly property bool away: entry.leaving === "away"

                width: Tokens.notificationWidth
                height: (card.implicitHeight + Tokens.gap) * presence
                z: -presence

                NumberAnimation on presence {
                    id: motion
                    to: 1
                    duration: Tokens.durationPanel
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Tokens.easing
                    onFinished: {
                        if (slot.entry.leaving)
                            stack.gone(slot.key);
                    }
                }

                Connections {
                    target: stack
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
                            motion.stop();
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
                    count: slot.entry.count
                    iconSource: stack.iconSource
                    now: stack.now
                    opacity: slot.presence
                    enabled: !slot.entry.leaving
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
                    Component.onCompleted: stack.cards = [...stack.cards, card]
                    Component.onDestruction: stack.cards = [...stack.cards].filter(item => item !== card)

                    transform: [
                        // Folds toward the button over its top right corner.
                        Scale {
                            origin.x: card.width
                            origin.y: 0
                            xScale: slot.away ? 1 : Tokens.notificationFold + (1 - Tokens.notificationFold) * slot.presence
                            yScale: slot.away ? 1 : Tokens.notificationFoldHeight + (1 - Tokens.notificationFoldHeight) * slot.presence
                        },
                        Translate {
                            x: slot.away ? (1 - slot.presence) * Tokens.notificationAway : 0
                            y: slot.away ? 0 : -(1 - slot.presence) * Tokens.notificationRise
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
        color: Qt.rgba(Tokens.bg.r, Tokens.bg.g, Tokens.bg.b, Tokens.surfaceOpacity)
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
