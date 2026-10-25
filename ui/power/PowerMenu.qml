pragma ComponentBehavior: Bound

import QtQuick
import "../stage"
import "../theme"
import "service"
import "../daemon/protocol.js" as Protocol

// The power menu: a glass tab that grows out of the right screen edge,
// halfway down, with lock, suspend, log out, restart and shut down. Opened
// by the right pill's power button or `nova toggle power`; it holds the
// keyboard while open (PowerChoice has the keys). Its body runs one radius
// past the screen edge, so only its left corners show rounded and the blur
// region stays one rounded rect; two concave fillets curve it into the edge.
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
                menu.choice.finished(error);
                if (error)
                    console.warn(`nova: power menu: ${action}: ${error}`);
                else
                    menu.open = false;
            });
        }
        onCloseRequested: menu.open = false
    }
    // An action still running after the menu closed shows nothing, so it
    // keeps neither the surface mapped nor its spinner turning.
    readonly property bool showsRunning: open && choice.running !== ""
    moving: morph.moving || reveal.running || showsRunning || choice.counting

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
    }

    EdgeFillet {
        above: true
        visible: menu.fillet > 0
        x: menu.width - menu.radius - menu.fillet
        y: -menu.fillet
        width: menu.fillet
        height: menu.fillet
    }

    EdgeFillet {
        above: false
        visible: menu.fillet > 0
        x: menu.width - menu.radius - menu.fillet
        y: menu.height
        width: menu.fillet
        height: menu.fillet
    }

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
                delay: 0.5
            }

            Text {
                width: rows.width - 2 * rows.padding
                leftPadding: Tokens.pad + Tokens.labelNudge
                topPadding: Tokens.labelNudge
                bottomPadding: Tokens.labelNudge
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeTiny
                font.capitalization: Font.AllUppercase
                color: Tokens.fgDim
                text: qsTr("Session")
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
