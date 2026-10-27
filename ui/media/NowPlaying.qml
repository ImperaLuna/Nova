pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../stage"
import "../icons"
import "../text/files.js" as Files
import "../daemon/protocol.js" as Protocol
import "media.js" as Media
import "service"

// The now-playing pill: the playing app's icon, title, artist and a small
// equalizer, beside the AI usage pill. Clicking it grows it into a panel
// with the cover, the progress line, the controls, one chip per app that
// has media, and a row that goes to the app. The same morph as the AI usage
// pill (Morph), so one click opens and closes both. Only drawn; the player
// and its commands come from MediaService.
StagePanel {
    id: root

    name: Protocol.FEATURE_MEDIA
    // The player the pill shows (a `media` player from novad), or null.
    required property var player
    // Every player, in the order they appeared, for the chips.
    required property var players

    readonly property bool playing: Media.isPlaying(player)
    readonly property bool paused: player !== null && player.status === "paused"
    readonly property string statusKey: player ? player.status : ""
    // A paused player's pill stays for a minute, unless the panel is open;
    // a stopped one's goes at once.
    property bool lingerOver: false
    readonly property bool pillShown: player !== null && player.status !== "stopped" && (playing || open || !lingerOver)
    readonly property string appName: player ? Media.appName(player) : ""
    readonly property string iconSource: player && player.app ? AppIcons.source(player.app.icon ?? "") : ""
    readonly property string title: player ? player.title : ""
    readonly property string artist: player ? Media.pillArtist(player) : ""

    // The pill's pieces, and the panel's sizes, from the tokens.
    readonly property int artSize: Tokens.mediaArt
    readonly property int faceGap: Tokens.mediaFaceGap
    readonly property real titleWidth: titleSlot.width
    readonly property real artistWidth: artist !== "" ? Math.min(artistMetrics.advanceWidth, Tokens.mediaArtistWidth) : 0
    readonly property real equalizerWidth: 4 * Tokens.mediaEqBar + 3 * Tokens.mediaEqGap
    readonly property real pillWidth: Tokens.mediaFacePadLeft + artSize + faceGap + titleWidth + (artist !== "" ? faceGap + artistWidth : 0) + faceGap + equalizerWidth + Tokens.mediaFacePadRight
    readonly property real progressFraction: Media.fraction(positionUs, player ? player.length_us : 0)
    readonly property real positionUs: player ? Media.positionAt(player, nowMs) : 0

    // The panel's height for what it shows now: its header row and body.
    readonly property real bodyHeight: body.implicitHeight
    openHeight: Tokens.barHeight + bodyHeight + Tokens.mediaPanelBottom

    // Clock for the progress line: read once when the panel opens and then
    // once a second while the shown player plays.
    property real nowMs: Date.now()
    // Where the workspace of the app's window is, from novad, or "".
    property string workspace: ""

    onStatusKeyChanged: lingerOver = false
    onPillShownChanged: {
        if (pillShown)
            console.info("nova: now playing pill shown");
        else
            open = false;
    }
    onOpenChanged: {
        // The VM tests wait for these before taking a screenshot.
        console.info(`nova: now playing panel ${open ? "opened" : "closed"}`);
        if (open) {
            nowMs = Date.now();
            workspace = "";
            if (player)
                MediaService.workspace(player.id, found => {
                    workspace = found;
                });
        }
    }
    onPlayerChanged: nowMs = Date.now()

    // The pill's own morph, from closed to the panel.
    Morph {
        id: morph
        open: root.open
    }
    readonly property real t: morph.t
    moving: morph.moving || levelsTimer.running || marquee.running

    // A paused player's pill stays for a minute, then goes, unless the panel is open.
    Timer {
        interval: Tokens.durationMediaLinger
        running: root.paused && !root.open && !root.lingerOver
        onTriggered: root.lingerOver = true
    }

    // The progress line counts up only while the panel is open and the player plays.
    Timer {
        interval: Tokens.durationProgressTick
        running: root.open && root.playing
        repeat: true
        onTriggered: root.nowMs = Date.now()
    }

    // The equalizer's bars step to new heights while the pill shows and plays.
    property var levels: [Tokens.mediaEqMin, Tokens.mediaEqMin, Tokens.mediaEqMin, Tokens.mediaEqMin]
    Timer {
        id: levelsTimer
        interval: Tokens.durationEqStep
        running: root.onScreen && root.playing && root.pillShown && !root.open
        repeat: true
        onTriggered: {
            const span = Tokens.mediaEqHeight - Tokens.mediaEqMin;
            root.levels = root.levels.map(() => Tokens.mediaEqMin + Math.round(Math.random() * span));
        }
    }

    // Whether the pill or panel is drawn at all. Its animations stop otherwise.
    readonly property bool onScreen: root.visible && root.opacity > 0

    // The title scrolls through once when a new track starts or when the pill
    // is hovered, then rests at its start. Never loops on its own. It moves
    // the title inside its slot, which clips it.
    property real titleScroll: 0
    SequentialAnimation {
        id: marquee
        running: root.onScreen && root.pillShown && !root.open && root.titleOverflow > 0 && root.scrollRequested
        onFinished: root.scrollRequested = false
        onRunningChanged: if (!running)
            root.titleScroll = 0
        PauseAnimation {
            duration: Tokens.durationMarqueePause
        }
        NumberAnimation {
            target: root
            property: "titleScroll"
            from: 0
            to: -root.titleOverflow
            duration: root.titleOverflow / Tokens.mediaMarqueeSpeed * Media.MILLISECONDS_PER_SECOND
        }
        PauseAnimation {
            duration: Tokens.durationMarqueePause
        }
    }
    property bool scrollRequested: false
    readonly property real titleOverflow: titleSlot.overflow
    onTitleChanged: scrollRequested = true

    TextMetrics {
        id: artistMetrics
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeSmall
        text: root.artist
    }

    GlassShadow {
        glass: surface
    }

    Glass {
        id: surface

        visible: root.pillShown || root.open
        width: morph.lerp(root.pillWidth, Tokens.mediaPanelWidth)
        height: morph.lerp(Tokens.barHeight, root.openHeight)
        radius: morph.lerp(Tokens.barHeight / 2, Tokens.radiusPanel)
        clip: true

        // The pill's face: it fades as the panel opens, and is clickable while closed.
        MouseArea {
            id: face
            width: surface.width
            height: Tokens.barHeight
            hoverEnabled: true
            enabled: !root.open
            cursorShape: Qt.PointingHandCursor
            onClicked: root.open = true
            onContainsMouseChanged: if (containsMouse)
                root.scrollRequested = true

            Accessible.role: Accessible.Button
            Accessible.name: `Open now playing, ${root.appName}: ${root.title}`

            Rectangle {
                anchors.fill: parent
                radius: height / 2
                color: Tokens.hover
                opacity: 1 - root.t
                visible: face.containsMouse && opacity > 0
            }
        }

        Item {
            id: art
            x: Tokens.mediaFacePadLeft
            y: (Tokens.barHeight - height) / 2
            width: root.artSize
            height: root.artSize
            opacity: (root.paused ? Tokens.mediaPausedArt : 1) * (1 - root.t)
            visible: opacity > 0

            Image {
                anchors.fill: parent
                visible: root.iconSource !== ""
                source: root.iconSource
                sourceSize: Qt.size(width * Screen.devicePixelRatio, height * Screen.devicePixelRatio)
                fillMode: Image.PreserveAspectFit
                asynchronous: true
                smooth: true
            }

            LineIcon {
                anchors.centerIn: parent
                visible: root.iconSource === ""
                path: Icons.MUSIC_NOTE
                size: root.artSize * Tokens.mediaNoteScale
                color: Tokens.fgDim
            }
        }

        // The title's slot: a long title is clipped at its end and faded out.
        TitleSlot {
            id: titleSlot
            x: Tokens.mediaFacePadLeft + root.artSize + root.faceGap
            text: root.title
            color: root.paused ? Tokens.fgDim : Tokens.fg
            scroll: root.titleScroll
            opacity: 1 - root.t
            visible: opacity > 0
        }

        Text {
            id: artistText
            x: titleSlot.x + titleSlot.width + root.faceGap
            y: (Tokens.barHeight - height) / 2
            width: root.artistWidth
            opacity: (1 - root.t) * (root.artist !== "" ? 1 : 0)
            visible: opacity > 0
            elide: Text.ElideRight
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeSmall
            color: Tokens.fgDim
            textFormat: Text.PlainText
            text: root.artist
        }

        // Four bars, 2 px apart, that move while playing and lie flat in fgDim when paused.
        Row {
            id: equalizer
            x: surface.width - Tokens.mediaFacePadRight - root.equalizerWidth
            y: (Tokens.barHeight - Tokens.mediaEqHeight) / 2
            spacing: Tokens.mediaEqGap
            height: Tokens.mediaEqHeight
            opacity: 1 - root.t
            visible: opacity > 0

            Repeater {
                model: 4

                Rectangle {
                    required property int index
                    anchors.bottom: parent.bottom
                    width: Tokens.mediaEqBar
                    height: root.paused ? Tokens.mediaEqMin : root.levels[index]
                    radius: width / 2
                    color: root.paused ? Tokens.fgDim : Tokens.primary
                }
            }
        }

        // The panel: the header row (closes it), then the body.
        MouseArea {
            id: header
            x: 0
            y: 0
            width: surface.width
            height: Tokens.barHeight
            visible: root.t > 0
            opacity: root.t
            onClicked: root.open = false

            Accessible.role: Accessible.Button
            Accessible.name: "Close now playing"

            Text {
                x: Tokens.mediaPanelPad
                anchors.verticalCenter: parent.verticalCenter
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: Font.DemiBold
                color: Tokens.fg
                textFormat: Text.PlainText
                text: root.appName
            }
        }

        Column {
            id: body
            x: Tokens.mediaPanelPad
            y: Tokens.barHeight
            width: Tokens.mediaPanelWidth - 2 * Tokens.mediaPanelPad
            spacing: Tokens.sectionGap
            opacity: root.t
            visible: root.t > 0

            Row {
                spacing: Tokens.mediaFaceGap

                Rectangle {
                    width: Tokens.mediaCover
                    height: Tokens.mediaCover
                    radius: Tokens.radiusRow
                    color: Tokens.surfaceRaised
                    clip: true

                    Image {
                        anchors.fill: parent
                        visible: root.player !== null && root.player.cover !== null
                        source: root.player && root.player.cover ? Files.fileUrl(root.player.cover) : ""
                        sourceSize: Qt.size(width * Screen.devicePixelRatio, height * Screen.devicePixelRatio)
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                    }

                    LineIcon {
                        anchors.centerIn: parent
                        visible: !root.player || root.player.cover === null
                        path: Icons.MUSIC_NOTE
                        size: Tokens.mediaCover * Tokens.mediaNoteScale
                        color: Tokens.fgDim
                    }
                }
            }

            Text {
                width: body.width
                maximumLineCount: Tokens.mediaTitleLines
                wrapMode: Text.Wrap
                elide: Text.ElideRight
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: Font.DemiBold
                color: Tokens.fg
                textFormat: Text.PlainText
                text: root.title
            }

            Text {
                width: body.width
                elide: Text.ElideRight
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                color: Tokens.fgDim
                textFormat: Text.PlainText
                text: root.artist
                visible: text !== ""
            }

            Text {
                width: body.width
                elide: Text.ElideRight
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                color: Tokens.fgDim
                textFormat: Text.PlainText
                text: root.player ? root.player.album : ""
                visible: text !== ""
            }

            // The progress line: click or drag to jump to a spot in the track.
            Item {
                id: progress
                width: body.width
                height: Tokens.mediaProgressHover
                enabled: root.player !== null && root.player.can_seek
                readonly property bool active: progressArea.containsMouse || progressArea.pressed

                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: parent.width
                    height: progressArea.containsMouse || progressArea.pressed ? Tokens.mediaProgressHover : Tokens.mediaProgress
                    radius: height / 2
                    color: Tokens.track

                    Rectangle {
                        height: parent.height
                        radius: parent.radius
                        width: parent.width * (progressArea.pressed ? progressArea.fraction : root.progressFraction)
                        color: Tokens.fg
                    }
                }

                MouseArea {
                    id: progressArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    property real fraction: 0
                    function fractionAt(x: real): real {
                        return Math.min(Math.max(x / width, 0), 1);
                    }
                    onPressed: mouse => fraction = fractionAt(mouse.x)
                    onPositionChanged: mouse => {
                        if (pressed)
                            fraction = fractionAt(mouse.x);
                    }
                    onReleased: mouse => {
                        fraction = fractionAt(mouse.x);
                        MediaService.seek(root.player.id, fraction * root.player.length_us);
                    }
                }
            }

            Row {
                width: body.width
                Text {
                    id: leftLabel
                    font.family: Tokens.fontMono
                    font.pixelSize: Tokens.fontSizeSmall
                    color: Tokens.fgDim
                    text: root.player ? Media.formatTime(root.positionUs) : ""
                }
                Item {
                    width: body.width - leftLabel.width - rightLabel.width
                    height: 1
                }
                Text {
                    id: rightLabel
                    font.family: Tokens.fontMono
                    font.pixelSize: Tokens.fontSizeSmall
                    color: Tokens.fgDim
                    text: root.player ? Media.formatTime(root.player.length_us) : ""
                }
            }

            // Previous, play or pause, next.
            Row {
                spacing: Tokens.mediaFaceGap
                anchors.horizontalCenter: parent.horizontalCenter

                ControlButton {
                    size: Tokens.mediaControl
                    path: Icons.PREVIOUS
                    enabled: root.player !== null && root.player.can_previous
                    accessibleName: "Previous"
                    onClicked: MediaService.control(root.player.id, Protocol.MEDIA_ACTION_PREVIOUS)
                }
                ControlButton {
                    size: Tokens.mediaPlay
                    path: root.playing ? Icons.PAUSE : Icons.PLAY
                    enabled: root.player !== null && (root.playing ? root.player.can_pause : root.player.can_play)
                    accessibleName: root.playing ? "Pause" : "Play"
                    onClicked: MediaService.control(root.player.id, Protocol.MEDIA_ACTION_PLAY_PAUSE)
                }
                ControlButton {
                    size: Tokens.mediaControl
                    path: Icons.NEXT
                    enabled: root.player !== null && root.player.can_next
                    accessibleName: "Next"
                    onClicked: MediaService.control(root.player.id, Protocol.MEDIA_ACTION_NEXT)
                }
            }

            // One chip per player, with a dot on the ones playing. Shown only with two or more.
            Flow {
                width: body.width
                spacing: Tokens.mediaChipGap
                visible: root.players.length > 1

                Repeater {
                    model: root.players

                    Rectangle {
                        id: chip
                        required property var modelData
                        readonly property bool selected: root.player !== null && modelData.id === root.player.id
                        height: Tokens.mediaChipHeight
                        width: chipRow.implicitWidth + 2 * Tokens.mediaChipPad
                        radius: height / 2
                        color: selected ? Tokens.fg : Tokens.mediaChip

                        Row {
                            id: chipRow
                            anchors.centerIn: parent
                            spacing: Tokens.mediaChipGap

                            Rectangle {
                                anchors.verticalCenter: parent.verticalCenter
                                visible: chip.modelData.status === "playing"
                                width: Tokens.mediaChipDot
                                height: Tokens.mediaChipDot
                                radius: width / 2
                                color: chip.selected ? Tokens.onAccent : Tokens.primary
                            }
                            Text {
                                font.family: Tokens.fontSans
                                font.pixelSize: Tokens.fontSizeSmall
                                color: chip.selected ? Tokens.onAccent : Tokens.fgDim
                                textFormat: Text.PlainText
                                text: Media.appName(chip.modelData)
                            }
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: MediaService.show(chip.modelData.id)
                        }
                    }
                }
            }

            // Goes to the app: brings its window forward and closes the panel.
            Rectangle {
                id: go
                width: body.width
                height: Tokens.mediaGoHeight
                radius: Tokens.radiusRow
                color: goArea.containsMouse ? Tokens.mediaGoHover : Tokens.surfaceRaised

                Text {
                    anchors.left: parent.left
                    anchors.leftMargin: Tokens.mediaPanelPad
                    anchors.verticalCenter: parent.verticalCenter
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSmall
                    font.weight: Font.Medium
                    color: Tokens.fg
                    textFormat: Text.PlainText
                    text: root.player ? `${Media.goToLabel(root.player, root.workspace)} ↗` : ""
                }

                MouseArea {
                    id: goArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        if (root.player)
                            MediaService.focus(root.player.id, null);
                        root.open = false;
                    }
                }
            }
        }
    }

    // A round transport button, drawn with a LineIcon.
    component ControlButton: MouseArea {
        id: control
        required property string path
        required property int size
        required property string accessibleName
        implicitWidth: size
        implicitHeight: size
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        opacity: enabled ? 1 : Tokens.dimOpacity

        Accessible.role: Accessible.Button
        Accessible.name: accessibleName

        Rectangle {
            anchors.fill: parent
            radius: height / 2
            color: control.containsMouse ? Tokens.hover : "transparent"
        }

        LineIcon {
            anchors.centerIn: parent
            path: control.path
            size: control.size * Tokens.mediaGlyphScale
            color: Tokens.fg
        }
    }
}
