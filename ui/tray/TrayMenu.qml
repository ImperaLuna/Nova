import QtQuick
import "../theme"
import "../stage"

// A tray app's menu under its icon in the right pill: glass on the stage,
// opened by a right click on the icon. It opens beside the tray (not
// exclusive), and closes with it: on Esc, a click outside, or once an
// entry ran.
StagePanel {
    id: menu

    name: "tray-menu"
    exclusive: false

    // The app (a TrayService entry) and its icon.
    property var entry: null
    property string iconSource: ""
    // The middle of the icon it opened from, and the screen's size.
    property real anchorX: 0
    property real screenWidth: 0
    property real screenHeight: 0

    signal done
    signal hideRequested(string key, bool hidden)

    radius: Tokens.radiusCard

    Morph {
        id: morph
        open: menu.open
    }
    moving: morph.moving

    // Right-aligned a little past its icon, kept on the screen.
    x: Math.max(Tokens.barMargin, Math.min(screenWidth - Tokens.barMargin - width, anchorX + Tokens.pillButtonSize / 2 + Tokens.menuGap - width))
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
        onDone: menu.done()
        onHideRequested: (key, hidden) => menu.hideRequested(key, hidden)
    }
}
