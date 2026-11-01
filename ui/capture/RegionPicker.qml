pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Effects
import "../theme"
import "../daemon/protocol.js" as Protocol
import "capture.js" as Capture
import "../text/files.js" as Files

// One screen's part of the region picker: its frozen frame under a dim
// veil, and the toolbar at the top (a switch to recording, Region or
// Screen, and close). In Region
// mode a drag outlines a region and letting go takes it, like Windows'
// Super+Shift+S; in Screen mode a click takes the whole screen. Esc or a
// right click closes it. It only asks; ScreenshotService does the rest.
//
// Picking what to record (`video`), the veil is over the live screen and
// the toolbar has the sound too. A region stays after the drag, with a
// Record button (or Enter) under it; in Screen mode a click records the
// screen. The count before it starts shows in the middle of what's
// recorded; RecordingService does the rest.
Item {
    id: picker

    // The frame's file (Daemon.capture's path for this screen); "" while
    // picking what to record.
    required property string framePath
    // A Protocol.CAPTURE_* mode, shared by every screen's picker.
    required property string mode
    // A pick is being saved: it takes no more.
    property bool busy: false
    // Picking what to record, and the sound for it.
    property bool video: false
    property bool desktopAudio: false
    property bool micAudio: false
    // The count to the recording's start, 0 when not counting.
    property int countdown: 0
    // The toolbar can switch between a screenshot and a recording, and was
    // opened by that switch.
    property bool canSwitch: false
    property bool switched: false

    // `rect` is in the frame's pixels, save_capture's rect.
    signal saveRequested(var rect)
    // `rect` is in logical pixels, null for the whole screen.
    signal recordRequested(var rect)
    signal cancelRequested
    signal modeRequested(string mode)
    // The toolbar's camera (false) or video (true).
    signal videoRequested(bool video)
    signal desktopAudioToggled
    signal micAudioToggled

    // The frame's pixels per logical pixel (the screen's scale).
    readonly property real frameScale: frame.implicitWidth > 0 && width > 0 ? frame.implicitWidth / width : 1
    // The region being dragged, in logical pixels, or null.
    property var region: null
    readonly property bool dragging: input.pressed && region !== null
    // The pointer is over this screen (Screen mode outlines all of it).
    // The toolbar too, so the outline doesn't drop out over it.
    readonly property bool hovered: hover.hovered
    // The toolbar's glass, the window's blur area.
    readonly property Item toolbarGlass: toolbar.glass
    // What this screen's picker asked to record (logical px), counting down
    // over it; null when it didn't.
    property var recorded: null
    readonly property bool counting: recorded !== null && countdown > 0
    property point _from

    // The switch showed this picker, already open but hidden (see
    // CaptureToolbar.arriveFrom).
    function arriveFrom(otherVideo: bool) {
        toolbar.arriveFrom(otherVideo);
    }

    // Handles a key; true when it was the picker's.
    function key(key: int): bool {
        if (key === Qt.Key_Escape) {
            cancelRequested();
            return true;
        }
        if (video && (key === Qt.Key_Return || key === Qt.Key_Enter) && region !== null && !busy) {
            _record(region);
            return true;
        }
        return false;
    }

    function _record(rect: var) {
        recorded = rect;
        recordRequested(mode === Protocol.CAPTURE_SCREEN ? null : rect);
    }

    function _take(rect: var) {
        const pixels = Capture.toPixels(rect, frameScale, frame.implicitWidth, frame.implicitHeight);
        if (pixels.width > 0 && pixels.height > 0)
            saveRequested(pixels);
    }

    onModeChanged: region = null

    HoverHandler {
        id: hover
    }

    // Cut out under the toolbar: its glass blurs the live screen there, as
    // when picking what to record, not the frame's copy of the pill it
    // stands in for.
    Image {
        id: frame
        anchors.fill: parent
        source: picker.framePath ? Files.fileUrl(picker.framePath) : ""
        asynchronous: true
        cache: false
        smooth: true
        layer.enabled: true
        layer.effect: MultiEffect {
            maskEnabled: true
            maskInverted: true
            maskSource: toolbarCutout
        }
    }

    Item {
        id: toolbarCutout
        width: picker.width
        height: picker.height
        visible: false
        layer.enabled: true

        Rectangle {
            x: toolbar.x
            y: toolbar.y
            width: toolbar.width
            height: toolbar.height
            radius: height / 2
            scale: picker.toolbarGlass.scale
        }
    }

    // The veil: around the region, or over everything but a hovered screen
    // in Screen mode.
    readonly property var _clear: {
        if (recorded)
            return Qt.rect(recorded.x, recorded.y, recorded.width, recorded.height);
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
        // A recording's size is in logical pixels, made even.
        readonly property var pixels: {
            if (!picker.region)
                return null;
            if (picker.video)
                return Capture.toLayout(picker.region, 0, 0);
            return Capture.toPixels(picker.region, picker.frameScale, frame.implicitWidth, frame.implicitHeight);
        }
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

    // Under the picked region (inside it when that's off the screen): starts
    // the recording.
    Rectangle {
        id: recordButton
        visible: picker.video && picker.region !== null && !picker.dragging && picker.recorded === null
        x: picker.region ? Math.min(picker.width - width, Math.max(0, picker.region.x + picker.region.width - width)) : 0
        y: {
            if (!picker.region)
                return 0;
            const below = picker.region.y + picker.region.height + Tokens.gap;
            return below + height <= picker.height ? below : picker.region.y + picker.region.height - height - Tokens.gap;
        }
        z: 1
        width: recordRow.implicitWidth + 2 * Tokens.captureButtonPad
        height: Tokens.captureButtonHeight
        radius: height / 2
        color: recordInput.containsMouse ? Tokens.surfaceRaised : Tokens.surface

        Row {
            id: recordRow
            anchors.centerIn: parent
            spacing: Tokens.iconGap

            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: Tokens.recordDotSize
                height: width
                radius: width / 2
                color: Tokens.error
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: qsTr("Record")
                textFormat: Text.PlainText
                color: Tokens.fg
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: Font.Medium
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: qsTr("Enter")
                textFormat: Text.PlainText
                color: Tokens.fgDim
                font.family: Tokens.fontMono
                font.pixelSize: Tokens.fontSizeSmall
            }
        }

        MouseArea {
            id: recordInput
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: picker._record(picker.region)
        }
    }

    // The count to the start, in the middle of what's recorded.
    Text {
        visible: picker.counting
        x: picker._clear.x + (picker._clear.width - width) / 2
        y: picker._clear.y + (picker._clear.height - height) / 2
        z: 1
        text: picker.countdown
        textFormat: Text.PlainText
        color: Tokens.fg
        style: Text.Outline
        styleColor: Tokens.captureVeil
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeCountdown
        font.weight: Font.DemiBold
    }

    MouseArea {
        id: input
        anchors.fill: parent
        enabled: !picker.busy && picker.recorded === null
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
                if (picker.video)
                    picker._record(Qt.rect(0, 0, picker.width, picker.height));
                else
                    picker._take(Qt.rect(0, 0, picker.width, picker.height));
                return;
            }
            const region = picker.region;
            // A click isn't a region: the picker stays for another try. A
            // region to record stays for its Record button.
            if (region && !Capture.isClick(region, Tokens.captureMinDrag)) {
                if (!picker.video)
                    picker._take(region);
            } else {
                picker.region = null;
            }
        }
    }

    CaptureToolbar {
        id: toolbar
        // Where it's centered with the sound buttons in, so it grows and
        // shrinks on the right.
        x: Math.round((picker.width - fullWidth) / 2)
        y: Tokens.barGap
        mode: picker.mode
        video: picker.video
        canSwitch: picker.canSwitch
        switched: picker.switched
        busy: picker.busy || picker.recorded !== null
        desktopAudio: picker.desktopAudio
        micAudio: picker.micAudio
        onVideoRequested: video => picker.videoRequested(video)
        onModeRequested: mode => picker.modeRequested(mode)
        onDesktopAudioToggled: picker.desktopAudioToggled()
        onMicAudioToggled: picker.micAudioToggled()
        onCloseRequested: picker.cancelRequested()
    }
}
