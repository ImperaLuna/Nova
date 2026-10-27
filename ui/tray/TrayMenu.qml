import QtQuick
import "../theme"
import "../stage"
import "../util/numbers.js" as Numbers

// A tray app's menu under its icon in the right pill: glass, opened by a right click on the icon. It opens beside the tray (not
// exclusive), and closes with it: on Esc, a click outside, or once an
// entry ran.
StagePanel {
    id: menu

    name: "tray-menu"
    exclusive: false
    // Below the top strip: on a surface of its own sized to it, so the
    // stage (and its blur) doesn't grow to a long menu's height.
    placement: "right"
    area: Qt.rect(x, y - Tokens.revealRise, width, height + Tokens.revealRise)

    // The app (a TrayService entry) and its icon.
    property var entry: null
    property string iconSource: ""
    // The middle of the icon it opened from, and the screen's size.
    property real anchorX: 0
    property real screenWidth: 0
    property real screenHeight: 0

    // TrayMenuList.runEntry.
    property var runEntry: null

    signal done
    signal hideRequested(string key, bool hidden)

    radius: Tokens.radiusCard

    Morph {
        id: morph
        open: menu.open
    }
    moving: morph.moving

    // Right-aligned a little past its icon, kept on the screen.
    x: Numbers.clamp(anchorX + Tokens.pillButtonSize / 2 + Tokens.menuGap - width, Tokens.barMargin, screenWidth - Tokens.barMargin - width)
    y: Tokens.barGap + Tokens.barHeight + Tokens.menuGap
    width: content.width
    height: content.implicitHeight
    openHeight: height
    visible: morph.t > 0
    opacity: morph.t
    // Drops a little into place.
    transform: Translate {
        y: morph.lerp(-Tokens.revealRise, 0)
    }

    GlassShadow {
        glass: surface
    }

    Glass {
        id: surface
        anchors.fill: parent
        radius: menu.radius
    }

    TrayMenuOpener {
        id: opener
        entry: menu.entry
    }

    TrayMenuList {
        id: content
        entry: menu.entry
        entries: opener.entries
        inSubmenu: opener.inSubmenu
        onSubmenuRequested: submenu => opener.open(submenu)
        onBackRequested: opener.back()
        iconSource: menu.iconSource
        // Kept on the screen; longer menus scroll.
        maxHeight: menu.screenHeight - menu.y - Tokens.barMargin
        runEntry: menu.runEntry
        onDone: menu.done()
        onHideRequested: (key, hidden) => menu.hideRequested(key, hidden)
    }
}
