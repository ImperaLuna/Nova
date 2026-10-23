pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import "../theme"
import "usage.js" as Usage

// The AI usage pill: the shown provider's logo and its session percent.
// Clicking it morphs the pill into the details panel in place, so the same
// click closes it again. Numbers come from novad's ai_usage event.
Item {
    id: root

    // Daemon.aiUsage: {shown, providers}.
    required property var usage
    property bool open: false

    // A row for another provider was clicked: show it in the bar.
    signal showRequested(string provider)
    // The panel opened: worth asking for fresh numbers.
    signal refreshRequested

    onOpenChanged: {
        if (open)
            refreshRequested();
    }

    readonly property var summary: Usage.summarize(Usage.shown(usage), now)
    readonly property bool hasIndicator: summary.state !== "ready"

    // Pill layout: pad, logo, gap, state icon, number, pad.
    readonly property int logoSize: 14
    readonly property int logoGap: 6
    readonly property int indicatorSize: 16
    readonly property int indicatorSpace: hasIndicator ? indicatorSize + logoGap : 0
    readonly property real pillWidth: Tokens.pad + logoSize + logoGap + indicatorSpace + heroClosedWidth + Tokens.pad
    // Panel layout.
    readonly property int panelWidth: 300
    readonly property int panelPad: 20
    readonly property int panelBottom: 16
    readonly property int heroTop: Tokens.barHeight + 8
    readonly property int heroSize: 56
    readonly property int heroLine: 60
    readonly property real percentSignOpen: 0.42
    readonly property int sectionGap: 18
    readonly property int rowHeight: 40
    readonly property int rowRadius: 14
    readonly property int sessionMeter: 8
    readonly property int weeklyMeter: 4
    readonly property int fadeDuration: 140
    readonly property int fillDuration: 600
    readonly property list<real> easing: [0.2, 0.8, 0.2, 1, 1, 1]

    // The panel's full height; the window keeps room for it.
    readonly property real openHeight: details.y + details.implicitHeight + panelBottom
    // 0 closed, 1 open: every size and position in between follows it.
    property real t: 0
    property real fill: 0
    property date now: new Date()

    readonly property real heroClosedScale: Tokens.fontSize / heroSize
    readonly property real heroClosedWidth: heroMetrics.advanceWidth

    function lerp(from: real, to: real): real {
        return from + (to - from) * t;
    }

    function percentColor(row: var): color {
        return row && row.warn ? Tokens.primary : Tokens.fg;
    }

    // The glass's corner radius, for the window's blur region.
    readonly property real radius: surface.radius

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
        interval: 30 * 1000
        running: root.open
        repeat: true
        triggeredOnStart: true
        onTriggered: root.now = new Date()
    }

    states: State {
        name: "open"
        when: root.open
        PropertyChanges {
            root.t: 1
            root.fill: 1
            name.opacity: 1
            plan.opacity: 1
            primaryBlock.opacity: 1
            secondBlock.opacity: 1
            othersBlock.opacity: 1
        }
    }

    transitions: [
        Transition {
            to: "open"
            ParallelAnimation {
                NumberAnimation {
                    target: root
                    property: "t"
                    duration: Tokens.durationPanel
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: root.easing
                }
                Fade {
                    target: name
                    delay: 0.4
                }
                Fade {
                    target: plan
                    delay: 0.55
                }
                Fade {
                    target: primaryBlock
                    delay: 0.55
                }
                Fade {
                    target: secondBlock
                    delay: 0.7
                }
                Fade {
                    target: othersBlock
                    delay: 0.85
                }
                SequentialAnimation {
                    PauseAnimation {
                        duration: Tokens.durationPanel * 0.6
                    }
                    NumberAnimation {
                        target: root
                        property: "fill"
                        duration: root.fillDuration
                        easing.type: Easing.BezierSpline
                        easing.bezierCurve: root.easing
                    }
                }
            }
        },
        Transition {
            from: "open"
            NumberAnimation {
                properties: "t,fill,opacity"
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: root.easing
            }
        }
    ]

    component Fade: SequentialAnimation {
        id: fade
        required property Item target
        // Start, as a share of the panel's morph.
        required property real delay
        PauseAnimation {
            duration: Tokens.durationPanel * fade.delay
        }
        NumberAnimation {
            target: fade.target
            property: "opacity"
            duration: root.fadeDuration
        }
    }

    component Meter: Rectangle {
        id: meter
        property var row
        implicitHeight: 4
        radius: height / 2
        color: Tokens.track

        Rectangle {
            height: parent.height
            radius: parent.radius
            width: parent.width * (meter.row ? meter.row.percent : 0) / 100 * root.fill
            color: meter.row && meter.row.warn ? Tokens.primary : Tokens.meter
        }
    }

    component Caption: Text {
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeSmall
        color: Tokens.fgDim
        visible: text !== ""
    }

    Rectangle {
        id: surface

        width: root.lerp(root.pillWidth, root.panelWidth)
        height: root.lerp(Tokens.barHeight, root.openHeight)
        radius: root.lerp(Tokens.barHeight / 2, Tokens.radiusPanel)
        color: Tokens.glass
        border.color: Tokens.glassRim
        clip: true

        MouseArea {
            id: pill
            width: root.pillWidth
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
            opacity: root.summary.state === "unavailable" ? 0.5 : 1
        }

        // Unavailable: a dashed ring. Loading: a spinning arc.
        Shape {
            x: Tokens.pad + root.logoSize + root.logoGap
            y: (Tokens.barHeight - height) / 2
            width: root.indicatorSize
            height: root.indicatorSize
            visible: root.hasIndicator
            opacity: 1 - root.t
            preferredRendererType: Shape.CurveRenderer

            ShapePath {
                strokeColor: Tokens.track
                strokeWidth: 1.5
                fillColor: "transparent"
                strokeStyle: root.summary.state === "unavailable" ? ShapePath.DashLine : ShapePath.SolidLine
                dashPattern: [2, 2]
                PathAngleArc {
                    centerX: 8
                    centerY: 8
                    radiusX: 6
                    radiusY: 6
                    sweepAngle: 360
                }
            }

            ShapePath {
                strokeColor: Tokens.fgDim
                strokeWidth: root.summary.state === "loading" ? 1.5 : 0
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                PathAngleArc {
                    centerX: 8
                    centerY: 8
                    radiusX: 6
                    radiusY: 6
                    sweepAngle: 86
                }
            }

            RotationAnimator on rotation {
                running: root.summary.state === "loading"
                from: 0
                to: 360
                duration: 1000
                loops: Animation.Infinite
            }
        }

        Text {
            id: name
            x: Tokens.pad + root.logoSize + root.logoGap + 1
            y: (Tokens.barHeight - height) / 2
            opacity: 0
            visible: opacity > 0
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            font.weight: Font.DemiBold
            color: Tokens.fg
            text: root.summary.name
        }

        Rectangle {
            id: plan
            anchors.right: parent.right
            anchors.rightMargin: 10
            y: (Tokens.barHeight - height) / 2
            height: 22
            width: planText.implicitWidth + 20
            radius: height / 2
            color: Tokens.surfaceRaised
            opacity: 0
            visible: opacity > 0 && root.summary.plan !== ""

            Text {
                id: planText
                anchors.centerIn: parent
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeTiny
                font.weight: Font.Medium
                color: Tokens.fgDim
                text: root.summary.plan
            }
        }

        // The pill's number, which grows into the panel's headline.
        Item {
            id: hero
            x: root.lerp(Tokens.pad + root.logoSize + root.logoGap + root.indicatorSpace, root.panelPad)
            y: root.lerp((Tokens.barHeight - root.heroLine * root.heroClosedScale) / 2, root.heroTop)
            width: number.width + percentSign.width
            height: root.heroLine
            scale: root.lerp(root.heroClosedScale, 1)
            transformOrigin: Item.TopLeft

            Text {
                id: number
                height: root.heroLine
                verticalAlignment: Text.AlignVCenter
                font.family: Tokens.fontMono
                font.pixelSize: root.heroSize
                font.weight: Font.Medium
                font.letterSpacing: root.lerp(0, -0.02 * root.heroSize)
                color: root.summary.main ? root.percentColor(root.summary) : Tokens.fgDim
                text: root.summary.hero

                Behavior on color {
                    ColorAnimation {
                        duration: 200
                    }
                }
            }

            Text {
                id: percentSign
                x: number.width
                anchors.baseline: number.baseline
                visible: root.summary.main !== null
                font.family: Tokens.fontMono
                font.pixelSize: root.heroSize * root.lerp(1, root.percentSignOpen)
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
                spacing: 8
                opacity: 0

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
                    lineHeight: 1.5
                    text: root.summary.message
                }
            }

            Column {
                id: secondBlock
                width: parent.width
                spacing: 8
                opacity: 0
                visible: root.summary.second !== null

                Item {
                    width: parent.width
                    height: secondName.height

                    Text {
                        id: secondName
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        color: Tokens.fg
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

            // The other providers; clicking one shows it in the bar instead.
            Column {
                id: othersBlock
                width: parent.width
                spacing: 8
                opacity: 0

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

                        x: -10
                        width: parent.width + 20
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
                            x: 10
                            anchors.verticalCenter: parent.verticalCenter
                            width: 16
                            provider: row.info.id
                            opacity: row.info.state === "unavailable" ? 0.5 : 1
                        }

                        Text {
                            x: 36
                            anchors.verticalCenter: parent.verticalCenter
                            font.family: Tokens.fontSans
                            font.pixelSize: Tokens.fontSize
                            color: Tokens.fg
                            text: row.info.name
                        }

                        Text {
                            anchors.right: parent.right
                            anchors.rightMargin: 10
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
}
