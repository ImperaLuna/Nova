pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../stage"
import "../theme"

// Placeholder panels, one per way a panel can sit off the stage, to try
// PanelSurface out before the real ones exist (IMP-67): a full-height
// sidebar and a tab on the right edge, and a deck in the middle of the
// screen that grows out of the central pill. Plain glass with a name, not
// designed UI. Only with NOVA_DEMO_PANELS=1 (ui/stage/Stage.qml), opened with
// `quickshell ipc --pid <UI pid> call demo toggle <name>`.
QtObject {
    id: demo

    required property ShellScreen screen
    // The deck grows out of it.
    required property Item centralPill

    readonly property list<StagePanel> panels: [sidebar, tab, deck]

    // Placeholder sizes, roughly the combo mockup's.
    readonly property int sidebarWidth: 380
    readonly property int tabWidth: 72
    readonly property int tabHeight: 300
    readonly property int deckWidth: 640
    readonly property int deckHeight: 400
    // The sidebar starts below the bar.
    readonly property int belowBar: Tokens.barGap + Tokens.barHeight + Tokens.barMargin

    // A glass box with its name, growing with its morph.
    component Placeholder: StagePanel {
        id: panel

        readonly property Morph morph: Morph {
            open: panel.open
        }
        moving: morph.moving

        onOpenChanged: console.info(`nova: demo panel ${name} ${open ? "opened" : "closed"}`)

        Glass {
            anchors.fill: parent
            radius: panel.radius
        }

        Text {
            anchors.centerIn: parent
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            color: Tokens.fgDim
            textFormat: Text.PlainText
            text: panel.name
            opacity: panel.open ? 1 : 0
            MorphReveal on opacity {
                delay: 0.5
            }
        }
    }

    // Slides in from the right edge, full height below the bar.
    property Placeholder sidebar: Placeholder {
        readonly property real openX: demo.screen.width - demo.sidebarWidth - Tokens.barMargin
        name: "demo-sidebar"
        placement: "right"
        radius: Tokens.radiusPanel
        x: morph.lerp(demo.screen.width, openX)
        y: demo.belowBar
        width: demo.sidebarWidth
        height: demo.screen.height - demo.belowBar - Tokens.barMargin
        area: Qt.rect(openX, y, width, height)
    }

    // Grows out of the right edge, halfway down.
    property Placeholder tab: Placeholder {
        name: "demo-tab"
        placement: "right"
        radius: Tokens.radiusPanel
        width: morph.lerp(0, demo.tabWidth)
        height: demo.tabHeight
        x: demo.screen.width - width
        y: (demo.screen.height - height) / 2
        area: Qt.rect(demo.screen.width - demo.tabWidth, y, demo.tabWidth, height)
    }

    // Grows out of the central pill into the middle of the screen, and takes
    // typing like the launcher will.
    property Placeholder deck: Placeholder {
        readonly property Item pill: demo.centralPill
        readonly property rect openRect: Qt.rect((demo.screen.width - demo.deckWidth) / 2, (demo.screen.height - demo.deckHeight) / 2, demo.deckWidth, demo.deckHeight)
        name: "demo-center"
        placement: "center"
        wantsKeyboard: true
        from: pill
        x: morph.lerp(pill.x, openRect.x)
        y: morph.lerp(pill.y, openRect.y)
        width: morph.lerp(pill.width, openRect.width)
        height: morph.lerp(pill.height, openRect.height)
        radius: morph.lerp(pill.height / 2, Tokens.radiusPanel)
        area: {
            const left = Math.min(pill.x, openRect.x);
            const top = Math.min(pill.y, openRect.y);
            return Qt.rect(left, top, Math.max(pill.x + pill.width, openRect.x + openRect.width) - left, Math.max(pill.y + pill.height, openRect.y + openRect.height) - top);
        }
    }
}
