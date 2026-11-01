pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../stage"
import "../daemon/protocol.js" as Protocol
import "media.js" as Media

// The now-playing pill: while a media player plays, it slides out with the
// app's icon, the title, the artist and an equalizer. Clicking it morphs it
// into the panel in place: the track's cover and names, a progress line to
// seek on, the buttons, chips to switch players, and a row going to the
// player's window. The panel's whole header row closes it again, as does a
// click outside. Paused, it dims and slides away after a minute unless the
// panel is open; a player that stops or goes takes it away at once. What it
// shows comes from novad's media event; it only draws and asks.
StagePanel {
    id: root

    name: Protocol.FEATURE_MEDIA
    // MediaService.media: {players, shown}.
    required property var media
    // Still equalizer bars, for screenshots that mustn't change.
    property bool still: false
    // The workspace the shown player's window is on, "" when unknown. Asked
    // for (workspaceAsked) each time the panel opens.
    property string workspace: ""

    signal controlRequested(string player, string action)
    signal seekRequested(string player, real positionUs)
    signal showRequested(string player)
    signal focusRequested(string player)
    signal workspaceAsked(string player)

    // The player novad shows, or null.
    readonly property var player: Media.shown(media)
    // What's drawn: the shown player, or the last one while it slides away.
    property var last: null
    readonly property var current: player ?? last
    readonly property bool playing: Media.playing(current)
    readonly property bool browser: current?.browser ?? false
    readonly property string appIcon: current?.app?.icon ?? ""
    readonly property string appName: Media.appName(current)

    // Paused for a minute: slid away, unless the panel is open.
    property bool lingered: false
    // The pill is wanted out.
    readonly property bool out: player !== null && player.status !== Protocol.PLAYBACK_STOPPED && !lingered
    // 0 hidden, 1 out: the pill slides out from nothing.
    property real shownT: out || open ? 1 : 0
    Behavior on shownT {
        NumberAnimation {
            id: reveal
            duration: Tokens.durationPanel
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }
    onShownTChanged: {
        // The VM tests wait for these before taking a screenshot.
        if (shownT === 1)
            console.info("nova: now playing pill shown");
        else if (shownT === 0)
            console.info("nova: now playing pill hidden");
    }

    // A new player or a new status restarts the linger.
    readonly property string playback: player ? `${player.id} ${player.status}` : ""
    onPlaybackChanged: rearm()
    function rearm() {
        lingered = false;
        if (player && player.status === Protocol.PLAYBACK_PAUSED && !open)
            linger.restart();
        else
            linger.stop();
    }
    Timer {
        id: linger
        interval: Tokens.durationMediaLinger
        onTriggered: root.lingered = !root.open
    }

    onPlayerChanged: {
        if (player)
            last = player;
        else
            open = false;
    }
    // A different player shown while open: ask where its window is.
    readonly property string playerId: player?.id ?? ""
    onPlayerIdChanged: {
        workspace = "";
        if (open && playerId !== "")
            workspaceAsked(playerId);
    }
    onOpenChanged: {
        console.info(`nova: now playing panel ${open ? "opened" : "closed"}`);
        if (open) {
            workspace = "";
            now = Date.now();
            if (playerId !== "")
                workspaceAsked(playerId);
        }
        rearm();
    }

    // The open panel's time counts up while it plays.
    property real now: Date.now()
    Timer {
        interval: Tokens.durationProgressStep
        running: root.open && root.playing
        repeat: true
        onTriggered: root.now = Date.now()
    }
    onCurrentChanged: now = Date.now()

    // The pill's layout: art, title, artist, equalizer.
    readonly property bool showsArtist: Media.pillArtist(current)
    readonly property real faceWidth: Tokens.mediaPillStart + Tokens.mediaArt + Tokens.gap + title.width + (showsArtist ? Tokens.gap + artist.width : 0) + Tokens.gap + Tokens.mediaEqLead + equalizer.implicitWidth + Tokens.pad
    property real pillWidth: faceWidth
    Behavior on pillWidth {
        enabled: root.shownT === 1
        NumberAnimation {
            id: resize
            duration: Tokens.durationResize
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }
    readonly property int panelWidth: Tokens.panelWidth
    readonly property int panelPad: Tokens.panelPad
    readonly property int contentWidth: panelWidth - 2 * panelPad
    readonly property PanelContent shownPanel: panel.item as PanelContent
    openHeight: shownPanel ? shownPanel.contentBottom + Tokens.mediaPadBottom : Tokens.barHeight

    Morph {
        id: morph
        open: root.open
    }
    readonly property real t: morph.t
    moving: morph.moving || reveal.running || resize.running || title.moving || dim.running || (shownPanel?.revealing ?? false)

    visible: shownT > 0
    opacity: shownT
    radius: surface.radius
    implicitWidth: surface.width
    implicitHeight: surface.height

    // Text from players is shown as written: never as markup.
    component Label: Text {
        textFormat: Text.PlainText
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSize
        color: Tokens.fg
        elide: Text.ElideRight
    }

    // A round button in the panel: the controls.
    component RoundButton: MouseArea {
        id: button
        property string icon: ""
        property string iconFill: ""
        property int iconSize: Tokens.mediaControlIcon
        property bool filled: false
        property string label: ""
        implicitWidth: Tokens.mediaControl
        implicitHeight: implicitWidth
        hoverEnabled: true
        cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
        opacity: enabled ? 1 : Tokens.mediaDisabledOpacity

        Accessible.role: Accessible.Button
        Accessible.name: label

        Rectangle {
            anchors.fill: parent
            radius: width / 2
            color: button.filled ? (button.containsMouse ? Tokens.primary : Tokens.alpha(Tokens.primary, Tokens.accentFillOpacity)) : Tokens.hover
            visible: button.filled || (button.containsMouse && button.enabled)
        }

        LineIcon {
            anchors.centerIn: parent
            path: button.icon
            fill: button.iconFill
            size: button.iconSize
            color: button.filled ? Tokens.onAccent : Tokens.fg
        }
    }

    GlassShadow {
        glass: surface
    }

    Glass {
        id: surface

        width: morph.lerp(root.pillWidth * root.shownT, root.panelWidth)
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
            Accessible.name: `${root.open ? "Close" : "Open"} now playing, ${root.current?.title ?? ""}${root.playing ? "" : ", paused"}`

            Rectangle {
                anchors.fill: parent
                radius: height / 2
                color: Tokens.hover
                // Only on the closed pill: open, it would sit behind the header.
                opacity: 1 - root.t
                visible: pill.containsMouse && opacity > 0
            }
        }

        // The pill's face, which fades as the panel grows.
        Item {
            id: face
            width: root.faceWidth
            height: Tokens.barHeight
            opacity: 1 - root.t
            visible: opacity > 0

            MediaArt {
                id: art
                x: Tokens.mediaPillStart
                anchors.verticalCenter: parent.verticalCenter
                size: Tokens.mediaArt
                pictureSize: Tokens.mediaAppIcon
                backed: false
                path: root.appIcon
                opacity: root.playing ? 1 : Tokens.mediaPausedOpacity
                Behavior on opacity {
                    NumberAnimation {
                        id: dim
                        duration: Tokens.durationColor
                    }
                }
            }

            MarqueeTitle {
                id: title
                x: art.x + art.width + Tokens.gap
                anchors.verticalCenter: parent.verticalCenter
                text: root.current?.title ?? ""
                color: root.playing ? Tokens.fg : Tokens.fgDim
                running: root.shownT === 1 && !root.open
                hovered: pill.containsMouse
                // A new track scrolls through once, once its text is in.
                onTextChanged: Qt.callLater(title.scroll)
                onRunningChanged: {
                    if (running)
                        Qt.callLater(title.scroll);
                }
            }

            Label {
                id: artist
                x: title.x + title.width + Tokens.gap
                anchors.verticalCenter: parent.verticalCenter
                width: Math.min(implicitWidth, Tokens.mediaArtistMax)
                visible: root.showsArtist
                font.pixelSize: Tokens.fontSizeSmall
                color: Tokens.fgDim
                text: root.current?.artist ?? ""
            }

            Equalizer {
                id: equalizer
                x: (root.showsArtist ? artist.x + artist.width : title.x + title.width) + Tokens.gap + Tokens.mediaEqLead
                anchors.verticalCenter: parent.verticalCenter
                playing: root.playing
                running: !root.open
                still: root.still
            }
        }

        // The panel's content, which fades in once it has grown. Loaded
        // only while the panel shows, so a closed pill keeps none of it and
        // a player's every change doesn't redraw it unseen.
        Loader {
            id: panel
            active: root.open || root.t > 0
            sourceComponent: Component {
                PanelContent {}
            }
        }
    }

    component PanelContent: Item {
        // Where its content ends, for the panel's height.
        readonly property real contentBottom: content.y + content.implicitHeight
        readonly property bool revealing: contentReveal.running

        width: root.panelWidth
        height: root.openHeight
        // Comes in hidden, then fades in.
        opacity: 0
        Component.onCompleted: opacity = Qt.binding(() => root.open ? 1 : 0)
        MorphReveal on opacity {
            id: contentReveal
            delay: Tokens.revealContent
        }
        visible: opacity > 0

        // The header: the app, plain, since the whole row closes the
        // panel (the pill's MouseArea, under this).
        Row {
            x: root.panelPad
            height: Tokens.barHeight
            spacing: Tokens.iconGap

            MediaArt {
                anchors.verticalCenter: parent.verticalCenter
                size: Tokens.iconSizeSmall
                backed: false
                path: root.appIcon
            }

            Label {
                anchors.verticalCenter: parent.verticalCenter
                width: Math.min(implicitWidth, root.contentWidth - Tokens.iconSizeSmall - Tokens.iconGap)
                font.weight: Font.Medium
                color: Tokens.fgDim
                text: root.appName
            }
        }

        Column {
            id: content
            x: root.panelPad
            y: Tokens.barHeight + Tokens.mediaGap
            width: root.contentWidth

            // The cover beside the title, artist and album.
            Item {
                width: parent.width
                height: Math.max(cover.height, meta.implicitHeight)

                MediaArt {
                    id: cover
                    anchors.verticalCenter: parent.verticalCenter
                    size: Tokens.mediaCover
                    radius: Tokens.radiusCard
                    path: root.current?.cover ?? ""
                }

                Column {
                    id: meta
                    x: cover.width + Tokens.mediaBodyGap
                    anchors.verticalCenter: parent.verticalCenter
                    width: parent.width - x
                    spacing: Tokens.mediaMetaGap

                    Label {
                        width: parent.width
                        font.pixelSize: Tokens.fontSizeTitle
                        font.weight: Font.DemiBold
                        lineHeight: Tokens.mediaTitleLine
                        wrapMode: Text.Wrap
                        maximumLineCount: 2
                        text: root.current?.title ?? ""
                    }

                    Label {
                        width: parent.width
                        color: Tokens.fgDim
                        visible: text !== ""
                        text: root.current?.artist ?? ""
                    }

                    Label {
                        width: parent.width
                        font.pixelSize: Tokens.fontSizeSmall
                        color: Tokens.fgDim
                        visible: text !== ""
                        text: root.current?.album ?? ""
                    }
                }
            }

            Item {
                width: 1
                height: Tokens.sectionGap
            }

            SeekLine {
                width: parent.width
                player: root.current
                now: root.now
                onSeekRequested: positionUs => root.seekRequested(root.current.id, positionUs)
            }

            Item {
                width: 1
                height: Tokens.mediaGap
            }

            Row {
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: Tokens.mediaControlGap

                RoundButton {
                    anchors.verticalCenter: parent.verticalCenter
                    icon: Icons.SKIP_PREVIOUS
                    iconFill: Icons.SKIP_PREVIOUS_FILL
                    label: "Previous track"
                    enabled: root.current?.can_previous ?? false
                    onClicked: root.controlRequested(root.current.id, Protocol.MEDIA_PREVIOUS)
                }

                RoundButton {
                    anchors.verticalCenter: parent.verticalCenter
                    implicitWidth: Tokens.mediaPlay
                    filled: true
                    icon: root.playing ? Icons.PAUSE_SOLID : Icons.PLAY_SOLID
                    iconFill: icon
                    iconSize: Tokens.mediaPlayIcon
                    label: root.playing ? "Pause" : "Play"
                    enabled: root.playing ? (root.current?.can_pause ?? false) : (root.current?.can_play ?? false)
                    onClicked: root.controlRequested(root.current.id, Protocol.MEDIA_PLAY_PAUSE)
                }

                RoundButton {
                    anchors.verticalCenter: parent.verticalCenter
                    icon: Icons.SKIP_NEXT
                    iconFill: Icons.SKIP_NEXT_FILL
                    label: "Next track"
                    enabled: root.current?.can_next ?? false
                    onClicked: root.controlRequested(root.current.id, Protocol.MEDIA_NEXT)
                }
            }

            // With two or more players, one chip each to show it instead.
            Item {
                width: 1
                height: Tokens.mediaRowGap
                visible: chips.count > 0
            }

            Flow {
                width: parent.width
                spacing: Tokens.mediaChipGap
                visible: chips.count > 0

                Repeater {
                    id: chips
                    model: Media.chips(root.media)

                    MouseArea {
                        id: chip
                        required property var modelData
                        readonly property bool chosen: modelData.id === root.current?.id

                        width: chipRow.implicitWidth + 2 * Tokens.mediaChipPad
                        height: Tokens.mediaChipHeight
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.showRequested(modelData.id)

                        Accessible.role: Accessible.Button
                        Accessible.name: `Show ${Media.appName(modelData)}${Media.playing(modelData) ? ", playing" : ""}`

                        Rectangle {
                            anchors.fill: parent
                            radius: height / 2
                            color: chip.chosen ? Tokens.fg : chip.containsMouse ? Tokens.selected : Tokens.surfaceRaised
                        }

                        Row {
                            id: chipRow
                            anchors.centerIn: parent
                            spacing: Tokens.iconGap

                            Rectangle {
                                anchors.verticalCenter: parent.verticalCenter
                                width: Tokens.mediaChipDot
                                height: width
                                radius: width / 2
                                visible: Media.playing(chip.modelData)
                                color: chip.chosen ? Tokens.onAccent : Tokens.primary
                            }

                            Label {
                                anchors.verticalCenter: parent.verticalCenter
                                font.pixelSize: Tokens.fontSizeSmall
                                font.weight: Font.Medium
                                color: chip.chosen ? Tokens.onAccent : Tokens.fgDim
                                text: Media.appName(chip.modelData)
                            }
                        }
                    }
                }
            }

            Item {
                width: 1
                height: Tokens.mediaRowGap
            }

            // Brings the player's window forward, then closes.
            MouseArea {
                id: goTo
                x: -Tokens.rowInset
                width: parent.width + 2 * Tokens.rowInset
                height: Tokens.rowHeight
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    root.focusRequested(root.current.id);
                    root.open = false;
                }

                Accessible.role: Accessible.Button
                Accessible.name: `Go to ${root.appName}${root.workspace !== "" ? ` on workspace ${root.workspace}` : ""}`

                Rectangle {
                    anchors.fill: parent
                    radius: Tokens.radiusRow
                    color: goTo.containsMouse ? Tokens.selected : Tokens.surfaceRaised
                }

                MediaArt {
                    id: goToIcon
                    x: Tokens.rowInset
                    anchors.verticalCenter: parent.verticalCenter
                    size: Tokens.mediaGoToIcon
                    backed: false
                    path: root.appIcon
                }

                Label {
                    x: goToIcon.x + goToIcon.width + Tokens.rowInset
                    anchors.verticalCenter: parent.verticalCenter
                    width: where.x - x - Tokens.rowInset
                    font.weight: Font.Medium
                    text: `Go to ${root.appName}`
                }

                Label {
                    id: where
                    anchors.right: arrow.left
                    anchors.rightMargin: Tokens.rowInset
                    anchors.verticalCenter: parent.verticalCenter
                    width: Math.min(implicitWidth, goTo.width / 2)
                    font.pixelSize: Tokens.fontSizeTiny
                    color: Tokens.fgDim
                    text: root.workspace !== "" ? `workspace ${root.workspace}` : ""
                }

                LineIcon {
                    id: arrow
                    anchors.right: parent.right
                    anchors.rightMargin: Tokens.rowInset
                    anchors.verticalCenter: parent.verticalCenter
                    path: Icons.GO_TO
                    size: Tokens.iconSizeSmall
                    color: goTo.containsMouse ? Tokens.fg : Tokens.fgDim
                }
            }
        }
    }
}
