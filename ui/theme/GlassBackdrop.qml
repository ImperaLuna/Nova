import QtQuick

// A copy of the screen behind a window's glass, as GlassTint reads it
// (GlassBackdrops). stage/Backdrop.qml takes the copies.
QtObject {
    // The window it draws in: a texture only draws in the window it was
    // made in.
    property QtObject window: null
    // The screen, as a texture source.
    property Item texture: null
    // A copy has come.
    property bool ready: false
    // The window's top-left on the screen.
    property point origin: Qt.point(0, 0)
    property real screenWidth: 0
    property real screenHeight: 0
    // The screen's top rows that are Nova's own glass (the bar's), never
    // looked at.
    property real skipTop: 0
}
