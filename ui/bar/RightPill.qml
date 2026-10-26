pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../stage"
import "../tray"
import "../tray/tray.js" as Tray
import "../daemon/protocol.js" as Protocol
import "../util/numbers.js" as Numbers

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
    property int unread: 0
    property bool doNotDisturb: false

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
    moving: morph.moving || strip.moving || scrolling.running || countReveal.running || countLeft.fading || countRight.fading

    // The strip's width open: up to trayVisible icons.
    readonly property int stripIcons: Math.min(trayApps.length, Tokens.trayVisible)
    readonly property real stripWidth: stripIcons > 0 ? stripIcons * Tokens.pillButtonSize + (stripIcons - 1) * Tokens.pillButtonGap : 0
    // With more than that, a "+N" on each side of the strip keeps its room.
    readonly property bool hasMore: trayApps.length > Tokens.trayVisible
    readonly property real moreWidth: hasMore ? countRight.width + Tokens.pillButtonGap : 0
    readonly property real trayOpenWidth: stripWidth + 2 * moreWidth + Tokens.pillButtonGap
    // Icons out of sight on each side ({left, right}), as the strip scrolls.
    readonly property var outOfSight: Tray.outOfSight(trayApps.length, Tokens.trayVisible, strip.contentX / strip.step)
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

            // Icons scrolled out of sight on each side, for the fades.
            readonly property real step: Tokens.pillButtonSize + Tokens.pillButtonGap
            readonly property bool fadesLeft: contentX > 0
            readonly property bool fadesRight: contentX < contentWidth - width - 1

            // The wheel scrolls the strip a whole icon per notch (a
            // touchpad's small turns add up to one), counting from where a
            // scroll still running is going, so quick turns add up.
            property real wheelTurn: 0
            WheelHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                onWheel: event => {
                    strip.wheelTurn += event.angleDelta.y !== 0 ? event.angleDelta.y : event.angleDelta.x;
                    const notches = Math.trunc(strip.wheelTurn / Tokens.wheelNotch);
                    if (notches === 0)
                        return;
                    strip.wheelTurn -= notches * Tokens.wheelNotch;
                    const from = scrolling.running ? scroll.targetValue : strip.contentX;
                    const target = from - notches * strip.step;
                    strip.contentX = Numbers.clamp(Math.round(target / strip.step) * strip.step, 0, strip.contentWidth - strip.width);
                }
            }

            Behavior on contentX {
                id: scroll
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

        // How many icons are out of sight on each side of the strip: on the
        // right at the start, on both sides in the middle, on the left at
        // the end.
        TrayCount {
            id: countLeft
            objectName: "countLeft"
            hidden: pill.outOfSight.left
            most: pill.trayApps.length - Tokens.trayVisible
            visible: pill.hasMore && morph.t > 0
            x: strip.x - Tokens.pillButtonGap - width
            anchors.verticalCenter: parent.verticalCenter
            opacity: pill.open ? 1 : 0
            MorphReveal on opacity {
                id: countReveal
                delay: Tokens.revealPillCount
            }
        }

        TrayCount {
            id: countRight
            objectName: "countRight"
            hidden: pill.outOfSight.right
            most: countLeft.most
            visible: countLeft.visible
            x: chevron.x - Tokens.pillButtonGap - width
            anchors.verticalCenter: parent.verticalCenter
            opacity: countLeft.opacity
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
            unread: pill.unread
            doNotDisturb: pill.doNotDisturb
            onQuickSettingsClicked: pill.quickSettingsClicked()
            onPowerClicked: pill.powerClicked()
        }
    }
}
