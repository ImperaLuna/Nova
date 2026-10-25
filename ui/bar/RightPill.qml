pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../stage"
import "../tray"
import "../tray/tray.js" as Tray
import "../daemon/protocol.js" as Protocol

// The right pill: the tray's chevron while some app sits in the tray, then
// the quick settings button and the orange power button (PillButtons),
// each shown while its feature is on. Glass, like the other pills. The
// chevron opens the tray in place: the pill grows to the left and the
// apps' icons pop in, at most Tokens.trayVisible of them, the rest a wheel
// scroll away with a "+N" for those out of sight. Opening the tray is
// opening this panel (StagePanel `tray`), so Esc or a click outside closes
// it, and so does opening quick settings, which grows out of the pill and
// hides it meanwhile.
StagePanel {
    id: pill

    name: Protocol.FEATURE_TRAY

    property bool quickSettings: false
    property bool power: false
    // Its panel is open on this screen: the button shows it's picked.
    property bool quickSettingsOpen: false
    // The tray's apps for the bar (TrayService.shown), and whether any asks
    // for attention.
    property var trayApps: []
    property bool trayAttention: false
    // What an Image shows for an app (TrayService.iconSource).
    property var iconSource: entry => ""

    signal quickSettingsClicked
    signal powerClicked
    // An app's icon was clicked; `at` is its middle, in this pill.
    signal trayActivated(var entry, point at)
    signal trayMenuRequested(var entry, point at)
    signal traySecondaryActivated(var entry)

    readonly property bool hasTray: trayApps.length > 0
    readonly property bool hasButtons: quickSettings || power
    // Never open with nothing to show.
    onHasTrayChanged: {
        if (!hasTray)
            open = false;
    }

    radius: height / 2

    Morph {
        id: morph
        open: pill.open
    }
    moving: morph.moving || strip.moving || scrolling.running

    // The strip's width open: up to trayVisible icons.
    readonly property int stripIcons: Math.min(trayApps.length, Tokens.trayVisible)
    readonly property real stripWidth: stripIcons > 0 ? stripIcons * Tokens.pillButtonSize + (stripIcons - 1) * Tokens.pillButtonGap : 0
    readonly property real moreWidth: trayApps.length > Tokens.trayVisible ? more.width + Tokens.pillButtonGap : 0
    readonly property real trayOpenWidth: stripWidth + moreWidth + Tokens.pillButtonGap
    // With a divider before the buttons, when there are any.
    readonly property real chevronWidth: hasTray ? Tokens.pillButtonSize + (hasButtons ? 2 * Tokens.pillButtonGap + Tokens.dividerWidth : 0) : 0

    implicitWidth: 2 * Tokens.pillButtonInset + morph.lerp(0, trayOpenWidth) + chevronWidth + buttons.implicitWidth
    implicitHeight: Tokens.barHeight

    GlassShadow {
        glass: surface
    }

    Glass {
        id: surface
        anchors.fill: parent
        radius: pill.radius
    }

    // Everything but the glass, clipped to it while it grows.
    Item {
        anchors.fill: parent
        anchors.leftMargin: Tokens.pillButtonInset
        anchors.rightMargin: Tokens.pillButtonInset
        clip: morph.moving

        // The icons, scrolling sideways, their ends fading out where more
        // are out of sight.
        Flickable {
            id: strip
            objectName: "strip"
            visible: pill.hasTray && morph.t > 0
            // Slides out from behind the chevron as the pill grows.
            x: chevron.x - Tokens.pillButtonGap - pill.moreWidth - width
            anchors.verticalCenter: parent.verticalCenter
            width: pill.stripWidth
            height: Tokens.pillButtonSize
            contentWidth: row.implicitWidth
            contentHeight: height
            flickableDirection: Flickable.HorizontalFlick
            boundsBehavior: Flickable.StopAtBounds
            interactive: contentWidth > width
            clip: true
            opacity: morph.t

            // Icons scrolled out of sight on each side, for the fades and "+N".
            readonly property real step: Tokens.pillButtonSize + Tokens.pillButtonGap
            readonly property bool fadesLeft: contentX > 0
            readonly property bool fadesRight: contentX < contentWidth - width - 1

            // The wheel scrolls the strip a whole icon at a time.
            WheelHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                onWheel: event => {
                    const delta = event.angleDelta.y !== 0 ? event.angleDelta.y : event.angleDelta.x;
                    const target = strip.contentX + (delta < 0 ? strip.step : -strip.step);
                    strip.contentX = Math.max(0, Math.min(strip.contentWidth - strip.width, Math.round(target / strip.step) * strip.step));
                }
            }

            Behavior on contentX {
                NumberAnimation {
                    id: scrolling
                    duration: Tokens.durationFast
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Tokens.easing
                }
            }

            Row {
                id: row
                spacing: Tokens.pillButtonGap

                Repeater {
                    model: pill.trayApps

                    TrayIcon {
                        id: icon
                        required property var modelData
                        required property int index
                        objectName: `tray-${modelData.key}`
                        source: pill.iconSource(modelData)
                        attention: modelData.attention
                        // Each pops in a little after the one before.
                        scale: pill.open ? 1 : 0.6
                        opacity: pill.open ? 1 : 0
                        // Fades out over the strip's ends where more are
                        // out of sight (a mask would need a GPU).
                        readonly property real inView: x - strip.contentX
                        fade: Math.max(0, Math.min(1, strip.fadesLeft ? (inView + width / 2) / Tokens.trayFade : 1, strip.fadesRight ? (strip.width - inView - width / 2) / Tokens.trayFade : 1))
                        Behavior on scale {
                            SequentialAnimation {
                                PauseAnimation {
                                    duration: pill.open ? Tokens.trayStagger * (icon.index + 2) : 0
                                }
                                NumberAnimation {
                                    duration: Tokens.durationPanel
                                    easing.type: Easing.BezierSpline
                                    easing.bezierCurve: Tokens.easing
                                }
                            }
                        }
                        Behavior on opacity {
                            SequentialAnimation {
                                PauseAnimation {
                                    duration: pill.open ? Tokens.trayStagger * (icon.index + 2) : 0
                                }
                                NumberAnimation {
                                    duration: Tokens.durationFade
                                }
                            }
                        }
                        onActivated: pill.trayActivated(modelData, icon.mapToItem(pill, width / 2, height / 2))
                        onMenuRequested: pill.trayMenuRequested(modelData, icon.mapToItem(pill, width / 2, height / 2))
                        onSecondaryActivated: pill.traySecondaryActivated(modelData)
                    }
                }
            }
        }

        // How many icons are out of sight to the right.
        Text {
            id: more
            objectName: "more"
            readonly property int count: Tray.moreCount(pill.trayApps.length, Tokens.trayVisible, strip.contentX / strip.step)
            visible: pill.trayApps.length > Tokens.trayVisible && morph.t > 0
            x: chevron.x - Tokens.pillButtonGap - width
            anchors.verticalCenter: parent.verticalCenter
            // Keeps its room when all are in sight, so the pill doesn't jump.
            text: count > 0 ? `+${count}` : ""
            width: widest.width
            color: Tokens.fgDim
            font.family: Tokens.fontMono
            font.pixelSize: Tokens.fontSizeMicro
            opacity: pill.open ? 1 : 0
            MorphReveal on opacity {
                delay: 0.8
            }

            TextMetrics {
                id: widest
                font: more.font
                text: `+${pill.trayApps.length}`
            }
        }

        // The tray's chevron, pointing at where it opens; it turns as it does.
        Rectangle {
            id: chevron
            objectName: "chevron"
            visible: pill.hasTray
            x: (pill.hasButtons ? divider.x - Tokens.pillButtonGap : buttons.x) - width
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.pillButtonSize
            height: Tokens.pillButtonSize
            radius: width / 2
            color: chevronPress.containsMouse ? Tokens.hover : "transparent"

            Behavior on color {
                ColorAnimation {
                    duration: Tokens.durationFast
                }
            }

            LineIcon {
                anchors.centerIn: parent
                path: Icons.LEFT
                rotation: morph.t * 180
            }

            // An app asking for attention, while the tray is shut.
            Rectangle {
                anchors.top: parent.top
                anchors.right: parent.right
                anchors.margins: Tokens.trayDotRing
                width: Tokens.trayDotSize
                height: Tokens.trayDotSize
                radius: width / 2
                color: Tokens.primary
                border.width: Tokens.trayDotRing
                border.color: Tokens.bg
                scale: pill.trayAttention && !pill.open ? 1 : 0

                Behavior on scale {
                    NumberAnimation {
                        duration: Tokens.durationPanel
                        easing.type: Easing.BezierSpline
                        easing.bezierCurve: Tokens.easing
                    }
                }
            }

            MouseArea {
                id: chevronPress
                anchors.fill: parent
                hoverEnabled: true
                onClicked: pill.open = !pill.open
            }
        }

        Rectangle {
            id: divider
            visible: pill.hasTray && pill.hasButtons
            x: buttons.x - Tokens.pillButtonGap - width
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.dividerWidth
            height: Tokens.dividerHeight
            color: Tokens.glassRim
        }

        PillButtons {
            id: buttons
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            quickSettings: pill.quickSettings
            power: pill.power
            quickSettingsOpen: pill.quickSettingsOpen
            onQuickSettingsClicked: pill.quickSettingsClicked()
            onPowerClicked: pill.powerClicked()
        }
    }
}
