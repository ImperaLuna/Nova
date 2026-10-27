pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Effects
import "../theme"
import "../theme/icons.js" as Icons
import "../text/files.js" as Files

// A picture in a rounded box: the image at `path` once it loads, else a
// music note on a quiet square. For the app's icon (`backed: false`: no
// box behind it) and the track's cover in the panel (cut to the box's
// rounding).
Item {
    id: art

    // An absolute file path, or "" for none.
    property string path: ""
    property int size: Tokens.mediaArt
    // How big the picture is inside the box: a cover fills it.
    property int pictureSize: size
    property real radius: size / 2
    // Whether the box is drawn behind the picture (a cover's), or only
    // behind the music note.
    property bool backed: true

    readonly property bool loaded: image.status === Image.Ready

    implicitWidth: size
    implicitHeight: size

    Rectangle {
        id: box
        anchors.fill: parent
        radius: art.radius
        color: Tokens.surfaceRaised
        visible: art.backed || !art.loaded
    }

    Image {
        id: image
        anchors.centerIn: parent
        width: art.pictureSize
        height: art.pictureSize
        source: art.path !== "" ? Files.fileUrl(art.path) : ""
        sourceSize: Qt.size(art.pictureSize * Screen.devicePixelRatio, art.pictureSize * Screen.devicePixelRatio)
        fillMode: art.backed ? Image.PreserveAspectCrop : Image.PreserveAspectFit
        asynchronous: true
        smooth: true
        visible: art.loaded
        // A cover takes the box's rounded corners.
        layer.enabled: art.backed && art.loaded
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: mask
        }
    }

    // The cover's shape, only ever drawn as its mask.
    Rectangle {
        id: mask
        width: art.pictureSize
        height: art.pictureSize
        radius: art.radius
        visible: false
        layer.enabled: true
    }

    LineIcon {
        anchors.centerIn: parent
        visible: !art.loaded
        path: Icons.MUSIC
        size: Math.round(art.size / 2)
        color: Tokens.fgDim
    }
}
