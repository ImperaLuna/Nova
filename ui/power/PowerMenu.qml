pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../stage"
import "../theme"
import "service"
import "../daemon/protocol.js" as Protocol
import "../text/duration.js" as Duration

// The power menu: a glass tab that grows out of the right screen edge,
// halfway down, with lock, suspend, log out, restart and shut down. Opened
// by the right pill's power button or `nova toggle power`; it holds the
// keyboard while open (PowerChoice has the keys). Its body runs one radius
// past the screen edge, so only its left corners show rounded and the blur
// region stays one rounded rect; two concave fillets curve it into the edge,
// taking clicks and blur with it.
StagePanel {
    id: menu

    // The screen it's on (StageScreen).
    required property StageScreen output

    name: Protocol.FEATURE_POWER
    placement: "right"
    wantsKeyboard: true

    readonly property Morph morph: Morph {
        open: menu.open
    }
    readonly property PowerChoice choice: PowerChoice {
        options: PowerService.options
        onRunRequested: action => {
            console.info(`nova: power menu: ${action}`);
            PowerService.run(action, error => {
                // The menu closed before an action that ends the session;
                // it opens again to say why that didn't happen.
                if (error && menu.choice.leaves(action))
                    menu.open = true;
                menu.choice.finished(error);
                if (error)
                    console.warn(`nova: power menu: ${action}: ${error}`);
                else
                    menu.open = false;
            });
        }
        onLeaveRequested: {
            menu.open = false;
            menu.leaveIfClosed();
        }
        onCloseRequested: menu.open = false
    }

    // Closed, its last frame drawn: the action waiting for that runs once
    // the frame is on screen (FrameOut), so the session never ends on a
    // half-closed menu.
    readonly property bool closed: !open && !morph.moving && !reveal.running
    onClosedChanged: leaveIfClosed()
    function leaveIfClosed() {
        if (closed && choice.leaving !== "")
            out.request();
    }
    readonly property FrameOut out: FrameOut {
        window: menu.Window.window
        onPendingChanged: {
            if (!pending && menu.closed)
                menu.choice.left();
        }
    }
    // A frame that never comes out (the screen blanked, the compositor
    // stopped asking for frames) must not keep the session from ending:
    // past this, the action runs anyway.
    readonly property int leaveWaitMaxMs: 1500
    Timer {
        interval: menu.leaveWaitMaxMs
        running: menu.choice.leaving !== ""
        onTriggered: menu.choice.left()
    }
    // How long the system has been up, in whole minutes, ticking while the
    // menu is open.
    SystemClock {
        id: clock
        enabled: menu.open
        precision: SystemClock.Minutes
    }
    readonly property string uptime: Duration.uptime((clock.date.getTime() - PowerService.bootedAt) / Duration.SECOND_MS)

    // An action still running after the menu closed shows nothing, so it
    // keeps neither the surface mapped nor its spinner turning. One waiting
    // for the menu to close keeps it drawing until that's on screen.
    readonly property bool showsRunning: open && choice.running !== ""
    moving: morph.moving || reveal.running || showsRunning || choice.counting || choice.leaving !== ""

    // Open, it's as tall as its rows; closed, a nub at the edge.
    readonly property real openWidth: Tokens.powerMenuWidth
    readonly property real fullHeight: rows.implicitHeight
    readonly property real fillet: morph.lerp(0, Tokens.edgeTabFillet)

    radius: Math.min(Tokens.edgeTabRadius, height / 2)
    width: morph.lerp(0, openWidth) + radius
    height: morph.lerp(Tokens.edgeTabNub, fullHeight)
    x: output.width - morph.lerp(0, openWidth)
    y: (output.height - height) / 2
    area: Qt.rect(output.width - openWidth, (output.height - fullHeight) / 2 - Tokens.edgeTabFillet, openWidth + Tokens.edgeTabRadius, fullHeight + 2 * Tokens.edgeTabFillet)

    onOpenChanged: {
        console.info(`nova: power menu ${open ? "opened" : "closed"}`);
        if (open) {
            choice.reset();
            PowerService.refresh();
            keys.forceActiveFocus();
        } else {
            choice.disarm();
        }
    }

    Glass {
        anchors.fill: parent
        radius: menu.radius
        // The fillets' rims carry the outline on into the edge.
        rimEnd: menu.width - menu.radius - menu.fillet
    }

    // On its parent, in screen coordinates like the tab, so the input and
    // blur regions follow them as the tab moves.
    parts: [
        EdgeFillet {
            above: true
            parent: menu.parent
            visible: menu.fillet > 0
            x: menu.x + menu.width - menu.radius - menu.fillet
            y: menu.y - menu.fillet
            width: menu.fillet
            height: menu.fillet
        },
        EdgeFillet {
            above: false
            parent: menu.parent
            visible: menu.fillet > 0
            x: menu.x + menu.width - menu.radius - menu.fillet
            y: menu.y + menu.height
            width: menu.fillet
            height: menu.fillet
        }
    ]

    // The part on screen; the rows sit where they are once open and the tab
    // grows over them.
    Item {
        id: keys
        width: menu.width - menu.radius
        height: menu.height
        clip: true
        Keys.onPressed: event => {
            event.accepted = menu.choice.key(event.key, event.text, event.isAutoRepeat);
        }

        Column {
            id: rows
            x: parent.width - menu.openWidth
            y: (parent.height - height) / 2
            width: menu.openWidth
            padding: Tokens.pad
            spacing: Tokens.powerRowGap
            opacity: menu.open ? 1 : 0
            MorphReveal on opacity {
                id: reveal
                delay: Tokens.revealContent
            }

            // "Session", and how long the system has been up.
            Item {
                width: rows.width - 2 * rows.padding
                height: session.implicitHeight

                Text {
                    id: session
                    leftPadding: Tokens.pad + Tokens.labelNudge
                    topPadding: Tokens.labelNudge
                    bottomPadding: Tokens.labelNudge
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeTiny
                    font.capitalization: Font.AllUppercase
                    color: Tokens.fgDim
                    text: qsTr("Session")
                }

                Text {
                    anchors.right: parent.right
                    anchors.baseline: session.baseline
                    rightPadding: Tokens.pad
                    visible: PowerService.bootedAt > 0
                    font.family: Tokens.fontMono
                    font.pixelSize: Tokens.fontSizeTiny
                    color: Tokens.fgDim
                    text: menu.uptime
                }
            }

            Repeater {
                model: menu.choice.actions

                PowerRow {
                    required property var modelData
                    width: rows.width - 2 * rows.padding
                    action: modelData.action
                    label: modelData.label
                    doing: modelData.doing
                    selected: menu.choice.selected === action
                    available: menu.choice.available(action)
                    reason: menu.choice.reason(action)
                    armed: menu.choice.armed === action
                    armedLeft: menu.choice.armedLeft
                    running: menu.showsRunning && menu.choice.running === action
                    busy: menu.choice.running !== "" && !running
                    onHovered: menu.choice.hover(action)
                    onClicked: menu.choice.pick(action)
                }
            }

            Text {
                width: rows.width - 2 * rows.padding
                visible: text !== ""
                leftPadding: Tokens.pad + Tokens.labelNudge
                rightPadding: Tokens.pad
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeTiny
                color: Tokens.error
                wrapMode: Text.Wrap
                textFormat: Text.PlainText
                text: menu.choice.problem
            }
        }
    }
}
