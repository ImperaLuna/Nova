import QtQuick
import Quickshell
import "../theme"
import "paths.js" as Paths

// A Nova Files window: an app window the compositor tiles and rounds,
// glass all through, showing `folder` with `select` selected.
FloatingWindow {
    id: window

    required property string folder
    property var select: []

    title: Paths.folderName(folder)
    implicitWidth: Tokens.filesWindowWidth
    implicitHeight: Tokens.filesWindowHeight
    color: Tokens.glass
}
