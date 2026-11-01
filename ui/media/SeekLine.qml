import QtQuick
import "../theme"
import "media.js" as Media

// The panel's progress line with the times under it. Clicking or dragging
// on it seeks (`seekRequested` on release); hovered, it grows and shows
// its knob. A player that can't seek, or a track of unknown length, shows
// the line without taking clicks.
Column {
    id: seek

    required property var player
    // Now, in ms since 1970: the panel steps it while it plays.
    required property real now

    signal seekRequested(real positionUs)

    readonly property bool seekable: (player?.can_seek ?? false) && (player?.length_us ?? 0) > 0
    // Where a drag or a click put it, until the player says where it is.
    property real held: -1
    readonly property real fraction: held >= 0 ? held : Media.progress(player, now)
    readonly property bool lifted: area.containsMouse || area.pressed

    // The player's next word on where it is ends any held position, or a
    // while without one: a seek that failed.
    onPlayerChanged: {
        if (!area.pressed)
            held = -1;
    }
    Timer {
        id: letGo
        interval: Tokens.durationSeekHold
        onTriggered: seek.held = -1
    }

    spacing: Tokens.mediaTimesGap

    Item {
        width: seek.width
        height: Tokens.mediaTrack

        MouseArea {
            id: area
            // Taller than the line, so it's easy to hit.
            y: (parent.height - height) / 2
            width: parent.width
            height: Tokens.mediaSeekRoom
            enabled: seek.seekable
            hoverEnabled: true
            cursorShape: seek.seekable ? Qt.PointingHandCursor : Qt.ArrowCursor
            preventStealing: true

            function at(x: real): real {
                return Math.min(1, Math.max(0, x / width));
            }

            onPressed: mouse => {
                letGo.stop();
                seek.held = at(mouse.x);
            }
            onPositionChanged: mouse => {
                if (pressed)
                    seek.held = at(mouse.x);
            }
            onReleased: mouse => {
                seek.held = at(mouse.x);
                letGo.restart();
                seek.seekRequested(Media.seekTarget(seek.player, seek.held));
            }
            onCanceled: seek.held = -1

            Accessible.role: Accessible.Slider
            Accessible.name: "Position"
        }

        Rectangle {
            id: line
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width
            height: seek.lifted ? Tokens.mediaTrackHover : Tokens.mediaTrack
            radius: height / 2
            color: Tokens.track

            Rectangle {
                width: parent.width * seek.fraction
                height: parent.height
                radius: parent.radius
                color: Tokens.meter
            }
        }

        Rectangle {
            x: line.width * seek.fraction - width / 2
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.mediaKnob
            height: width
            radius: width / 2
            color: Tokens.fg
            visible: seek.lifted && seek.seekable
        }
    }

    Item {
        width: seek.width
        height: position.implicitHeight

        Text {
            id: position
            font.family: Tokens.fontMono
            font.pixelSize: Tokens.fontSizeSmall
            color: Tokens.fgDim
            text: Media.time(seek.held >= 0 ? Media.seekTarget(seek.player, seek.held) : Media.position(seek.player, seek.now))
        }

        Text {
            anchors.right: parent.right
            font.family: Tokens.fontMono
            font.pixelSize: Tokens.fontSizeSmall
            color: Tokens.fgDim
            visible: (seek.player?.length_us ?? 0) > 0
            text: Media.time(seek.player?.length_us ?? 0)
        }
    }
}
