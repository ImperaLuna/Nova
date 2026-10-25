pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../daemon/protocol.js" as Protocol
import "capture.js" as Capture

// One screen's part of the region picker: its frozen frame under a dim
// veil, and the toolbar at the top (Region or Screen, and close). In Region
// mode a drag outlines a region and letting go takes it, like Windows'
// Super+Shift+S; in Screen mode a click takes the whole screen. Esc or a
// right click closes it. It only asks; ScreenshotService does the rest.
Item {
    id: picker

    // The frame's file (Daemon.capture's path for this screen).
    required property string framePath
    // A Protocol.CAPTURE_* mode, shared by every screen's picker.
    required property string mode
    // A pick is being saved: it takes no more.
    property bool busy: false

    // `rect` is in the frame's pixels, save_capture's rect.
    signal saveRequested(var rect)
    signal cancelRequested
    signal modeRequested(string mode)

    // The frame's pixels per logical pixel (the screen's scale).
    readonly property real frameScale: frame.implicitWidth > 0 && width > 0 ? frame.implicitWidth / width : 1
    // The region being dragged, in logical pixels, or null.
    property var region: null
    readonly property bool dragging: input.pressed && region !== null
    // The pointer is over this screen (Screen mode outlines all of it).
    readonly property bool hovered: input.containsMouse
    // The toolbar's glass, the window's blur area.
    readonly property Item toolbarGlass: toolbar.glass
    property point _from

    // Handles a key; true when it was the picker's.
    function key(key: int): bool {
        if (key === Qt.Key_Escape) {
            cancelRequested();
            return true;
        }
        return false;
    }

    function _take(rect: var) {
        const pixels = Capture.toPixels(rect, frameScale, frame.implicitWidth, frame.implicitHeight);
        if (pixels.width > 0 && pixels.height > 0)
            saveRequested(pixels);
    }

    onModeChanged: region = null

    Image {
        id: frame
        anchors.fill: parent
        source: picker.framePath ? Capture.fileUrl(picker.framePath) : ""
        asynchronous: true
        cache: false
        smooth: true
    }

    // The veil: around the region, or over everything but a hovered screen
    // in Screen mode.
    readonly property var _clear: {
        if (mode === Protocol.CAPTURE_SCREEN)
            return hovered ? Qt.rect(0, 0, width, height) : Qt.rect(0, 0, 0, 0);
        return region ? Qt.rect(region.x, region.y, region.width, region.height) : Qt.rect(0, 0, 0, 0);
    }
    Repeater {
        model: [Qt.rect(0, 0, picker.width, picker._clear.y), Qt.rect(0, picker._clear.y + picker._clear.height, picker.width, picker.height - picker._clear.y - picker._clear.height), Qt.rect(0, picker._clear.y, picker._clear.x, picker._clear.height), Qt.rect(picker._clear.x + picker._clear.width, picker._clear.y, picker.width - picker._clear.x - picker._clear.width, picker._clear.height)]
        Rectangle {
            required property rect modelData
            x: modelData.x
            y: modelData.y
            width: Math.max(0, modelData.width)
            height: Math.max(0, modelData.height)
            color: Tokens.captureVeil
        }
    }

    // The outline, around the region or the hovered screen.
    Rectangle {
        visible: picker._clear.width > 0 && picker._clear.height > 0
        x: picker._clear.x - Tokens.captureOutline
        y: picker._clear.y - Tokens.captureOutline
        width: picker._clear.width + 2 * Tokens.captureOutline
        height: picker._clear.height + 2 * Tokens.captureOutline
        color: "transparent"
        border.color: Tokens.primary
        border.width: Tokens.captureOutline
    }

    // The region's size in the frame's pixels, under its bottom-right
    // corner (inside it when that's off the screen).
    Rectangle {
        id: badge
        readonly property var pixels: picker.region ? Capture.toPixels(picker.region, picker.frameScale, frame.implicitWidth, frame.implicitHeight) : null
        visible: picker.dragging && pixels !== null
        x: picker.region ? Math.max(0, picker.region.x + picker.region.width - width) : 0
        y: {
            if (!picker.region)
                return 0;
            const below = picker.region.y + picker.region.height + Tokens.gap;
            return below + height <= picker.height ? below : picker.region.y + picker.region.height - height - Tokens.gap;
        }
        width: size.implicitWidth + 2 * Tokens.chipPad
        height: Tokens.captureBadgeHeight
        radius: height / 2
        color: Tokens.surface

        Text {
            id: size
            anchors.centerIn: parent
            text: badge.pixels ? Capture.size(badge.pixels.width, badge.pixels.height) : ""
            textFormat: Text.PlainText
            color: Tokens.fg
            font.family: Tokens.fontMono
            font.pixelSize: Tokens.fontSizeSmall
        }
    }

    MouseArea {
        id: input
        anchors.fill: parent
        enabled: !picker.busy
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        cursorShape: Qt.CrossCursor

        onPressed: mouse => {
            if (mouse.button === Qt.RightButton) {
                picker.cancelRequested();
                return;
            }
            picker._from = Qt.point(mouse.x, mouse.y);
            if (picker.mode === Protocol.CAPTURE_REGION)
                picker.region = Capture.region(mouse.x, mouse.y, mouse.x, mouse.y);
        }
        onPositionChanged: mouse => {
            if (pressed && picker.region !== null)
                picker.region = Capture.region(picker._from.x, picker._from.y, mouse.x, mouse.y);
        }
        onReleased: mouse => {
            if (mouse.button !== Qt.LeftButton)
                return;
            if (picker.mode === Protocol.CAPTURE_SCREEN) {
                picker._take(Qt.rect(0, 0, picker.width, picker.height));
                return;
            }
            const region = picker.region;
            // A click isn't a region: the picker stays for another try.
            if (region && !Capture.isClick(region, Tokens.captureMinDrag))
                picker._take(region);
            else
                picker.region = null;
        }
    }

    CaptureToolbar {
        id: toolbar
        anchors.horizontalCenter: parent.horizontalCenter
        y: Tokens.barGap
        mode: picker.mode
        onModeRequested: mode => picker.modeRequested(mode)
        onCloseRequested: picker.cancelRequested()
    }
}
