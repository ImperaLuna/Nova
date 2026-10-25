import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../volume"

// A thick slider: a round icon button beside the track (clicking it sends
// `iconClicked`, mute), the track filled only up to the value with a label
// and the percent on it, and with `opens` an arrow button after it that
// opens a page. Drag or click the track to set it. The icon is a volume
// glyph (VolumeIcon), a line icon, or an app's icon (`appIcon`, else its
// name's first letter, with `app`).
Item {
    id: slider

    // 0 to 1; more (another app set it above 100%) shows full.
    property real value: 0
    property string label: ""
    // volume.js icon() glyph, or "" for `icon`, a line icon path.
    property string glyph: ""
    property string icon: ""
    // An app's slider: its icon's file, or "" for its name's first letter.
    property bool app: false
    property string appIcon: ""
    property color fill: Tokens.sliderFill
    property color iconColor: Tokens.fg
    property bool available: true
    // Muted: the fill stays at the volume, dimmed.
    property bool muted: false
    property bool opens: false

    signal moved(real value)
    signal iconClicked
    signal opened

    implicitHeight: Tokens.sliderHeight
    opacity: available ? 1 : Tokens.dimOpacity

    // The track, in the slider's coordinates.
    readonly property real trackX: iconButton.width + Tokens.sliderGap
    readonly property real track: width - trackX - (opens ? Tokens.sliderButtonWidth + Tokens.sliderGap : 0)
    // While dragged, where the pointer put it: the value set comes back a
    // moment later, and showing that would jump behind the pointer.
    readonly property real current: drag.pressed ? drag.value : value
    readonly property real shown: Math.max(0, Math.min(1, current))

    Rectangle {
        id: iconButton
        objectName: "icon"
        width: Tokens.sliderHeight
        height: Tokens.sliderHeight
        radius: slider.app ? Tokens.appIconRadius : height / 2
        color: slider.app && slider.appIcon !== "" ? "transparent" : mute.containsMouse ? Tokens.hover : Tokens.track

        VolumeIcon {
            anchors.centerIn: parent
            visible: !slider.app && slider.glyph !== ""
            glyph: slider.glyph
            color: slider.muted ? Tokens.fgDim : slider.iconColor
        }
        LineIcon {
            anchors.centerIn: parent
            visible: !slider.app && slider.glyph === ""
            path: slider.icon
            color: slider.muted ? Tokens.fgDim : slider.iconColor
        }
        Image {
            id: appImage
            anchors.centerIn: parent
            visible: slider.app && status === Image.Ready
            width: Tokens.sliderHeight
            height: Tokens.sliderHeight
            sourceSize: Qt.size(width, height)
            fillMode: Image.PreserveAspectFit
            source: slider.appIcon
            asynchronous: true
            opacity: slider.muted ? Tokens.appMutedOpacity : 1
        }
        Text {
            anchors.centerIn: parent
            visible: slider.app && !appImage.visible
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeSmall
            font.weight: Font.DemiBold
            color: slider.muted ? Tokens.fgDim : Tokens.fg
            text: slider.label.charAt(0).toUpperCase()
        }
        MouseArea {
            id: mute
            anchors.fill: parent
            hoverEnabled: true
            enabled: slider.available
            onClicked: slider.iconClicked()
        }
    }

    Rectangle {
        id: background
        objectName: "track"
        x: slider.trackX
        width: slider.track
        height: parent.height
        radius: height / 2
        color: Tokens.track
        clip: true

        Rectangle {
            objectName: "fill"
            width: background.width * slider.shown
            height: parent.height
            radius: height / 2
            color: slider.fill
            opacity: slider.muted ? Tokens.sliderMutedOpacity : 1
        }

        Text {
            anchors.verticalCenter: parent.verticalCenter
            x: Tokens.pad
            width: number.x - x - Tokens.gap
            elide: Text.ElideRight
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeSmall
            color: Tokens.fg
            textFormat: Text.PlainText
            text: slider.label
        }

        Text {
            id: number
            visible: slider.available
            anchors.verticalCenter: parent.verticalCenter
            x: background.width - width - Tokens.pad
            font.family: Tokens.fontMono
            font.pixelSize: Tokens.fontSizeSmall
            color: slider.muted ? Tokens.fgDim : Tokens.fg
            text: Math.round(slider.current * 100)
        }

        MouseArea {
            id: drag
            anchors.fill: parent
            enabled: slider.available
            preventStealing: true
            property real value: 0
            function set(mouseX: real) {
                const next = Math.round(Math.max(0, Math.min(1, mouseX / width)) * 100) / 100;
                if (pressed && next === value)
                    return;
                value = next;
                slider.moved(next);
            }
            onPressed: mouse => set(mouse.x)
            onPositionChanged: mouse => {
                if (pressed)
                    set(mouse.x);
            }
        }
    }

    Rectangle {
        visible: slider.opens
        anchors.right: parent.right
        width: Tokens.sliderButtonWidth
        height: parent.height
        radius: height / 2
        color: open.containsMouse ? Tokens.hover : Tokens.track

        LineIcon {
            anchors.centerIn: parent
            path: Icons.RIGHT
        }

        MouseArea {
            id: open
            anchors.fill: parent
            hoverEnabled: true
            onClicked: slider.opened()
        }
    }
}
