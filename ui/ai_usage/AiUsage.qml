pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import "../theme"
import "../stage"
import "../daemon/protocol.js" as Protocol
import "usage.js" as Usage

// The AI usage pill: the shown provider's logo and its session percent.
// Clicking it morphs the pill into the details panel in place, so the same
// click closes it again, as does the panel's whole header or a click
// outside. Numbers come from novad's ai_usage event. A StagePanel, so the
// stage gives it clicks and blur.
StagePanel {
    id: root

    name: Protocol.FEATURE_AI_USAGE
    closesOnClickOutside: true
    // AiUsageService.usage: {shown, providers}.
    required property var usage

    // A row for another provider was clicked: show it in the bar.
    signal showRequested(string provider)
    // The panel opened: worth asking for fresh numbers.
    signal refreshRequested

    onOpenChanged: {
        // The VM tests wait for these before taking a screenshot.
        console.info(`nova: ai usage panel ${open ? "opened" : "closed"}`);
        if (open)
            refreshRequested();
    }

    readonly property var summary: Usage.summarize(Usage.shown(usage), now)
    readonly property bool hasIndicator: summary.state !== "ready"

    // Pill layout: pad, logo, gap, state icon, number, pad.
    readonly property int logoSize: Tokens.iconSizeSmall
    readonly property int logoGap: Tokens.iconGap
    readonly property int indicatorSize: Tokens.iconSize
    readonly property real indicatorRadius: indicatorSize / 2 - Tokens.ringInset
    readonly property int indicatorSpace: hasIndicator ? indicatorSize + logoGap : 0
    readonly property real pillWidth: Tokens.pad + logoSize + logoGap + indicatorSpace + heroClosedWidth + Tokens.pad
    // Panel layout, from the panel tokens.
    readonly property int panelWidth: Tokens.panelWidth
    readonly property int panelPad: Tokens.panelPad
    readonly property int panelBottom: Tokens.panelPadBottom
    readonly property int heroTop: Tokens.barHeight + Tokens.gap
    readonly property int heroSize: Tokens.fontSizeHero
    readonly property int heroLine: Tokens.lineHeightHero
    readonly property int sectionGap: Tokens.sectionGap
    readonly property int rowHeight: Tokens.rowHeight
    readonly property int rowRadius: Tokens.radiusRow
    readonly property int sessionMeter: Tokens.meterHeight
    readonly property int weeklyMeter: Tokens.meterHeightSmall
    readonly property int fillDuration: Tokens.durationFill
    // How often the "left" and "resets" texts count down.
    readonly property int countdownInterval: 30 * 1000

    // The panel's height for what it shows now.
    readonly property real contentHeight: details.y + details.implicitHeight + sectionGap + othersBlock.implicitHeight + panelBottom
    // The tallest it has been: it grows when a provider needs more room and
    // never shrinks back, so switching providers doesn't make it jump.
    property real tallest: 0
    onContentHeightChanged: tallest = Math.max(tallest, contentHeight)
    // The panel's full height.
    openHeight: Math.max(contentHeight, tallest)
    // The pill growing into the panel: every size and position in between
    // follows morph.t (0 closed, 1 open).
    Morph {
        id: morph
        open: root.open
    }
    readonly property real t: morph.t
    // The meters fill last as it opens and empty with the morph as it
    // closes, so their fill spans every content fade.
    moving: morph.moving || fillReveal.running || heroColor.running || spinner.running
    // The meters fill once the panel has grown, and empty as it closes.
    property real fill: root.open ? 1 : 0
    MorphReveal on fill {
        id: fillReveal
        delay: 0.6
        duration: root.fillDuration
        eased: true
    }
    property date now: new Date()

    readonly property real heroClosedScale: Tokens.fontSize / heroSize
    readonly property real heroClosedWidth: heroMetrics.advanceWidth

    function percentColor(row: var): color {
        return row && row.warn ? Tokens.primary : Tokens.fg;
    }

    radius: surface.radius

    implicitWidth: surface.width
    implicitHeight: surface.height

    TextMetrics {
        id: heroMetrics
        font.family: Tokens.fontMono
        font.pixelSize: Tokens.fontSize
        font.weight: Font.Medium
        text: root.summary.main ? root.summary.hero + "%" : root.summary.hero
    }

    // The "left" and "resets" texts count down only while they're visible.
    Timer {
        interval: root.countdownInterval
        running: root.open
        repeat: true
        triggeredOnStart: true
        onTriggered: root.now = new Date()
    }

    component Meter: Rectangle {
        id: meter
        property var row
        implicitHeight: Tokens.meterHeightSmall
        radius: height / 2
        color: Tokens.track

        Rectangle {
            height: parent.height
            radius: parent.radius
            width: parent.width * (meter.row ? meter.row.percent : 0) / 100 * root.fill
            color: meter.row && meter.row.warn ? Tokens.primary : Tokens.meter
        }
    }

    // Text from novad, like hints and plan names, is shown as written: never
    // as markup.
    component Caption: Text {
        textFormat: Text.PlainText
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeSmall
        color: Tokens.fgDim
        visible: text !== ""
    }

    GlassShadow {
        glass: surface
    }

    Glass {
        id: surface

        width: morph.lerp(root.pillWidth, root.panelWidth)
        height: morph.lerp(Tokens.barHeight, root.openHeight)
        radius: morph.lerp(Tokens.barHeight / 2, Tokens.radiusPanel)
        clip: true

        // The pill, which grows into the panel's whole header row.
        MouseArea {
            id: pill
            width: surface.width
            height: Tokens.barHeight
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.open = !root.open

            Accessible.role: Accessible.Button
            Accessible.name: `${root.open ? "Close" : "Open"} AI usage, ${root.summary.name}${root.summary.main ? ` ${root.summary.main.percent} percent` : ""}`

            Rectangle {
                anchors.fill: parent
                radius: height / 2
                color: Tokens.hover
                // Only on the closed pill: open, it would sit behind the title.
                opacity: 1 - root.t
                visible: pill.containsMouse && opacity > 0
            }
        }

        ProviderLogo {
            x: Tokens.pad
            y: (Tokens.barHeight - height) / 2
            width: root.logoSize
            provider: root.summary.id
            opacity: root.summary.state === "unavailable" ? Tokens.dimOpacity : 1
        }

        // Unavailable: a dashed ring. Loading: a spinning arc.
        Shape {
            id: indicator
            x: Tokens.pad + root.logoSize + root.logoGap
            y: (Tokens.barHeight - height) / 2
            width: root.indicatorSize
            height: root.indicatorSize
            visible: root.hasIndicator
            opacity: 1 - root.t
            preferredRendererType: Shape.CurveRenderer

            ShapePath {
                strokeColor: Tokens.track
                strokeWidth: Tokens.strokeThin
                fillColor: "transparent"
                strokeStyle: root.summary.state === "unavailable" ? ShapePath.DashLine : ShapePath.SolidLine
                dashPattern: Tokens.ringDashes
                PathAngleArc {
                    centerX: root.indicatorSize / 2
                    centerY: root.indicatorSize / 2
                    radiusX: root.indicatorRadius
                    radiusY: root.indicatorRadius
                    sweepAngle: 360
                }
            }

            ShapePath {
                strokeColor: Tokens.fgDim
                strokeWidth: root.summary.state === "loading" ? Tokens.strokeThin : 0
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                PathAngleArc {
                    centerX: root.indicatorSize / 2
                    centerY: root.indicatorSize / 2
                    radiusX: root.indicatorRadius
                    radiusY: root.indicatorRadius
                    sweepAngle: Tokens.spinnerSweep
                }
            }

            RotationAnimator on rotation {
                id: spinner
                // Only while it can be seen: opening the panel fades it out.
                running: root.summary.state === "loading" && indicator.visible && indicator.opacity > 0
                from: 0
                to: 360
                duration: Tokens.durationSpin
                loops: Animation.Infinite
            }
        }

        Text {
            id: name
            x: Tokens.pad + root.logoSize + root.logoGap + Tokens.labelNudge
            y: (Tokens.barHeight - height) / 2
            opacity: root.open ? 1 : 0
            MorphReveal on opacity {
                delay: 0.4
            }
            visible: opacity > 0
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            font.weight: Font.DemiBold
            color: Tokens.fg
            textFormat: Text.PlainText
            text: root.summary.name
        }

        Rectangle {
            id: plan
            anchors.right: parent.right
            anchors.rightMargin: Tokens.chipPad
            y: (Tokens.barHeight - height) / 2
            height: Tokens.chipHeight
            width: planText.implicitWidth + 2 * Tokens.chipPad
            radius: height / 2
            color: Tokens.surfaceRaised
            opacity: root.open ? 1 : 0
            MorphReveal on opacity {
                delay: 0.55
            }
            visible: opacity > 0 && root.summary.plan !== ""

            Text {
                id: planText
                anchors.centerIn: parent
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeTiny
                font.weight: Font.Medium
                color: Tokens.fgDim
                textFormat: Text.PlainText
                text: root.summary.plan
            }
        }

        // The pill's number, which grows into the panel's headline.
        Item {
            id: hero
            x: morph.lerp(Tokens.pad + root.logoSize + root.logoGap + root.indicatorSpace, root.panelPad)
            y: morph.lerp((Tokens.barHeight - root.heroLine * root.heroClosedScale) / 2, root.heroTop)
            width: number.width + percentSign.width
            height: root.heroLine
            scale: morph.lerp(root.heroClosedScale, 1)
            transformOrigin: Item.TopLeft

            Text {
                id: number
                height: root.heroLine
                verticalAlignment: Text.AlignVCenter
                font.family: Tokens.fontMono
                font.pixelSize: root.heroSize
                font.weight: Font.Medium
                font.letterSpacing: morph.lerp(0, Tokens.trackingHero * root.heroSize)
                color: root.summary.main ? root.percentColor(root.summary) : Tokens.fgDim
                textFormat: Text.PlainText
                text: root.summary.hero

                Behavior on color {
                    ColorAnimation {
                        id: heroColor
                        duration: Tokens.durationColor
                    }
                }
            }

            Text {
                id: percentSign
                x: number.width
                anchors.baseline: number.baseline
                visible: root.summary.main !== null
                font.family: Tokens.fontMono
                font.pixelSize: root.heroSize * morph.lerp(1, Tokens.heroUnitScale)
                font.weight: Font.Medium
                color: number.color
                text: "%"
            }
        }

        Column {
            id: details
            x: root.panelPad
            y: root.heroTop + root.heroLine
            width: root.panelWidth - root.panelPad * 2
            spacing: root.sectionGap

            // The bar's number: the 5-hour session, or the only window.
            Column {
                id: primaryBlock
                width: parent.width
                spacing: Tokens.gap
                opacity: root.open ? 1 : 0
                MorphReveal on opacity {
                    delay: 0.55
                }

                Item {
                    width: parent.width
                    height: primaryName.height
                    visible: root.summary.main !== null

                    Caption {
                        id: primaryName
                        text: root.summary.main ? root.summary.main.name : ""
                    }

                    Caption {
                        anchors.right: parent.right
                        font.family: Tokens.fontMono
                        text: root.summary.main && root.summary.second ? root.summary.main.left : ""
                    }
                }

                Meter {
                    width: parent.width
                    height: root.sessionMeter
                    visible: root.summary.main !== null
                    row: root.summary.main
                }

                // A lone window says when it resets under its meter.
                Caption {
                    text: root.summary.main && !root.summary.second ? root.summary.main.reset : ""
                }

                Caption {
                    width: parent.width
                    wrapMode: Text.Wrap
                    lineHeight: Tokens.lineHeightWrapped
                    text: root.summary.message
                }
            }

            Column {
                id: secondBlock
                width: parent.width
                spacing: Tokens.gap
                opacity: root.open ? 1 : 0
                MorphReveal on opacity {
                    delay: 0.7
                }
                visible: root.summary.second !== null

                Item {
                    width: parent.width
                    height: secondName.height

                    Text {
                        id: secondName
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        color: Tokens.fg
                        textFormat: Text.PlainText
                        text: root.summary.second ? root.summary.second.name : ""
                    }

                    Text {
                        anchors.right: parent.right
                        anchors.baseline: secondName.baseline
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSize
                        font.weight: Font.Medium
                        color: root.percentColor(root.summary.second)
                        text: root.summary.second ? `${root.summary.second.percent}%` : ""
                    }
                }

                Meter {
                    width: parent.width
                    height: root.weeklyMeter
                    row: root.summary.second
                }

                Caption {
                    text: root.summary.second ? root.summary.second.reset : ""
                }
            }
        }
        // The other providers; clicking one shows it in the bar instead.
        // Pinned to the panel's bottom, so switching providers never moves it.
        Column {
            id: othersBlock
            x: root.panelPad
            y: root.openHeight - root.panelBottom - height
            width: details.width
            spacing: Tokens.gap
            opacity: root.open ? 1 : 0
            MorphReveal on opacity {
                delay: 0.85
            }

            Rectangle {
                width: parent.width
                height: 1
                color: Tokens.outline
            }

            Repeater {
                model: Usage.others(root.usage)

                MouseArea {
                    id: row
                    required property var modelData
                    readonly property var info: Usage.summarize(modelData, root.now)

                    x: -Tokens.rowInset
                    width: parent.width + 2 * Tokens.rowInset
                    height: root.rowHeight
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.showRequested(row.info.id)

                    Accessible.role: Accessible.Button
                    Accessible.name: `Show ${row.info.name} in the bar${row.info.main ? `, ${row.info.main.percent} percent` : ""}`

                    Rectangle {
                        anchors.fill: parent
                        radius: root.rowRadius
                        color: Tokens.hover
                        visible: row.containsMouse
                    }

                    ProviderLogo {
                        x: Tokens.rowInset
                        anchors.verticalCenter: parent.verticalCenter
                        width: Tokens.iconSize
                        provider: row.info.id
                        opacity: row.info.state === "unavailable" ? Tokens.dimOpacity : 1
                    }

                    Text {
                        x: 2 * Tokens.rowInset + Tokens.iconSize
                        anchors.verticalCenter: parent.verticalCenter
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        color: Tokens.fg
                        textFormat: Text.PlainText
                        text: row.info.name
                    }

                    Text {
                        anchors.right: parent.right
                        anchors.rightMargin: Tokens.rowInset
                        anchors.verticalCenter: parent.verticalCenter
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSize
                        font.weight: Font.Medium
                        color: row.info.main ? root.percentColor(row.info) : Tokens.fgDim
                        text: row.info.main ? `${row.info.hero}%` : row.info.hero
                    }
                }
            }
        }
    }
}
