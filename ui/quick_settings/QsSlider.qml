import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../volume"

// A thick slider: a fill with a short knob inside its end, the icon, a
// label and the percent on top. Drag or click to set it. The icon is a
// volume glyph (VolumeIcon) or a line icon; clicking it sends
// `iconClicked` (mute). With `opens`, an arrow button at its end opens a
// page.
Item {
    id: slider

    // 0 to 1; more (another app set it above 100%) shows full.
    property real value: 0
    property string label: ""
    // volume.js icon() glyph, or "" for `icon`, a line icon path.
    property string glyph: ""
    property string icon: ""
    property color fill: Tokens.sliderFill
    property color iconColor: Tokens.fg
    property bool available: true
    property bool opens: false

    signal moved(real value)
    signal iconClicked
    signal opened

    implicitHeight: Tokens.sliderHeight
    opacity: available ? 1 : Tokens.dimOpacity

    readonly property real track: opens ? width - button.width - Tokens.segmentPad : width
    readonly property real shown: Math.max(Tokens.sliderMinFill, Math.min(1, value))

    Rectangle {
        id: background
        anchors.fill: parent
        radius: height / 2
        color: Tokens.track
        clip: true

        Rectangle {
            width: slider.track * slider.shown
            height: parent.height
            radius: height / 2
            color: slider.fill

            Rectangle {
                anchors.right: parent.right
                anchors.rightMargin: Tokens.sliderKnobInset
                y: Tokens.sliderKnobPad
                width: Tokens.sliderKnobWidth
                height: parent.height - 2 * Tokens.sliderKnobPad
                radius: width / 2
                color: Tokens.meter
            }
        }
    }

    MouseArea {
        id: drag
        width: slider.track
        height: parent.height
        enabled: slider.available
        preventStealing: true
        function set(mouseX: real) {
            slider.moved(Math.round(Math.max(0, Math.min(1, mouseX / width)) * 100) / 100);
        }
        onPressed: mouse => set(mouse.x)
        onPositionChanged: mouse => {
            if (pressed)
                set(mouse.x);
        }
    }

    Item {
        id: iconBox
        x: Tokens.pad
        anchors.verticalCenter: parent.verticalCenter
        width: Tokens.iconSize
        height: Tokens.iconSize

        VolumeIcon {
            visible: slider.glyph !== ""
            glyph: slider.glyph
            color: slider.iconColor
        }
        LineIcon {
            visible: slider.glyph === ""
            path: slider.icon
            color: slider.iconColor
        }
        MouseArea {
            anchors.fill: parent
            anchors.margins: -Tokens.labelNudge * 4
            enabled: slider.available
            onClicked: slider.iconClicked()
        }
    }

    Text {
        anchors.verticalCenter: parent.verticalCenter
        x: iconBox.x + iconBox.width + Tokens.gap
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
        x: slider.track - width - Tokens.pad
        font.family: Tokens.fontMono
        font.pixelSize: Tokens.fontSizeSmall
        color: Tokens.fg
        text: Math.round(slider.value * 100)
    }

    Rectangle {
        id: button
        visible: slider.opens
        anchors.right: parent.right
        anchors.rightMargin: Tokens.segmentPad
        anchors.verticalCenter: parent.verticalCenter
        width: slider.opens ? Tokens.sliderButtonWidth : 0
        height: Tokens.sliderButtonHeight
        radius: height / 2
        color: open.containsMouse ? Tokens.hover : "transparent"

        Rectangle {
            width: Tokens.dividerWidth
            height: parent.height
            color: Tokens.sliderDivider
        }

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
