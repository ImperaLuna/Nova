pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../text/files.js" as Files

// A picture filling its box, cropped to fit as awww draws a wallpaper, that
// fades to the next once that has loaded, so stepping through images never
// shows a blank. Moving on before one has loaded loads the newest instead.
Item {
    id: preview

    // An absolute file path, or "" for none.
    property string path: ""
    // How big to decode it: the screen's size in pixels, never the file's.
    required property size pixels
    readonly property bool moving: fade.running

    // The layer showing a picture, and the one the next loads into, on top.
    property Image front: first
    readonly property Image back: front === first ? second : first

    onPathChanged: load()
    Component.onCompleted: load()

    function load() {
        if (fade.running) {
            fade.stop();
            swap();
        }
        // None: both go at once, so nothing is held while it's out of sight.
        if (path === "") {
            first.source = "";
            second.source = "";
            return;
        }
        const source = Files.fileUrl(path);
        if (front.source.toString() === source) {
            back.source = "";
            return;
        }
        back.opacity = 0;
        back.z = 1;
        front.z = 0;
        back.source = source;
        if (back.status === Image.Ready)
            fade.start();
    }

    // The back layer has the picture: it fades in over the front one.
    function loaded(layer: Image) {
        if (layer === back && layer.status === Image.Ready && !fade.running)
            fade.start();
    }

    // Once faded in, the back layer is the front, and the old picture goes.
    function swap() {
        const old = front;
        front = back;
        old.source = "";
    }

    NumberAnimation {
        id: fade
        target: preview.back
        property: "opacity"
        to: 1
        duration: Tokens.durationPreview
        easing.type: Easing.BezierSpline
        easing.bezierCurve: Tokens.easing
        onFinished: preview.swap()
    }

    PreviewLayer {
        id: first
    }

    PreviewLayer {
        id: second
    }

    component PreviewLayer: Image {
        anchors.fill: parent
        sourceSize: preview.pixels
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        // Decoded at the screen's size: kept only while shown.
        cache: false
        onStatusChanged: preview.loaded(this)
    }
}
