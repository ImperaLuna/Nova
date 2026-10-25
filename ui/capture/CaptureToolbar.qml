pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../daemon/protocol.js" as Protocol
import "capture.js" as Capture

// The region picker's toolbar, where the central pill sits: a switch
// between a screenshot and a recording (when both are on), what to take (a
// region or the whole screen), for a recording the sound too (the
// desktop's, the mic), and close. It grows in from the pill's height when
// the picker opens. The sound buttons slide in on the right, so whatever is
// left of them stays put: the picker places it by `fullWidth`, its width
// with them. `glass` is its blur area.
Item {
    id: toolbar

    required property string mode
    // Picking what to record: the sound buttons show.
    property bool video: false
    property bool desktopAudio: false
    property bool micAudio: false
    // Screenshots and recordings are both on and nothing records: the
    // switch shows. Not while a pick is busy.
    property bool canSwitch: false
    property bool busy: false
    // Opened by the switch on the other kind's toolbar: it starts as that
    // one was, already grown in, and moves to its own kind from there.
    property bool switched: false
    signal videoRequested(bool video)
    signal modeRequested(string mode)
    signal desktopAudioToggled
    signal micAudioToggled
    signal closeRequested

    readonly property Item glass: surface
    // Its width with the sound buttons in, wherever they are now.
    readonly property real fullWidth: implicitWidth - extras.width + (canSwitch || video ? extras.shownWidth : 0)
    // The kind it shows: `video`, after starting from the other one when
    // `switched`.
    property bool _shownVideo: switched ? !video : video

    implicitWidth: row.implicitWidth + extras.width + Tokens.barGap + tail.implicitWidth + 2 * Tokens.barGap
    implicitHeight: Tokens.captureBarHeight

    // Set for a moment while it jumps to the other kind, unanimated.
    property bool _snap: false

    Component.onCompleted: Qt.callLater(() => {
        toolbar._shownVideo = Qt.binding(() => toolbar.video);
    })

    // Shows it at once as the other kind's toolbar was, then moves to its
    // own: the switch showed this one's picker, already open but hidden.
    function arriveFrom(otherVideo: bool) {
        _snap = true;
        _shownVideo = otherVideo;
        _snap = false;
        Qt.callLater(() => {
            toolbar._shownVideo = Qt.binding(() => toolbar.video);
        });
    }

    GlassShadow {
        glass: surface
    }

    Glass {
        id: surface
        anchors.centerIn: parent
        width: parent.width
        height: parent.height
        radius: height / 2
        scale: shown ? 1 : Tokens.barHeight / Tokens.captureBarHeight
        opacity: shown ? 1 : 0
        property bool shown: toolbar.switched
        Component.onCompleted: shown = true

        Behavior on scale {
            NumberAnimation {
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
        Behavior on opacity {
            NumberAnimation {
                duration: Tokens.durationFade
            }
        }

        Row {
            id: row
            anchors.verticalCenter: parent.verticalCenter
            x: Tokens.barGap
            spacing: Tokens.barGap

            CaptureSwitch {
                anchors.verticalCenter: parent.verticalCenter
                visible: toolbar.canSwitch
                enabled: !toolbar.busy
                video: toolbar._shownVideo
                animated: !toolbar._snap
                onToggled: toolbar.videoRequested(!toolbar.video)
            }

            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                visible: toolbar.canSwitch
                width: Tokens.dividerWidth
                height: Tokens.dividerHeight
                color: Tokens.outline
            }

            Repeater {
                model: [
                    {
                        mode: Protocol.CAPTURE_REGION,
                        label: qsTr("Region")
                    },
                    {
                        mode: Protocol.CAPTURE_SCREEN,
                        label: qsTr("Screen")
                    }
                ]
                CaptureButton {
                    required property var modelData
                    anchors.verticalCenter: parent.verticalCenter
                    icon: modelData.mode
                    label: modelData.label
                    radius: height / 2
                    selected: toolbar.mode === modelData.mode
                    onClicked: toolbar.modeRequested(modelData.mode)
                }
            }
        }

        // The sound buttons, sliding in from the right for a recording,
        // with the gap before them inside: nothing is left of them when
        // they're out. Not in the row, which would drop the gap with them.
        Item {
            id: extras
            readonly property real shownWidth: Tokens.barGap + extrasRow.implicitWidth
            anchors.verticalCenter: parent.verticalCenter
            x: row.x + row.width
            width: toolbar._shownVideo ? shownWidth : 0
            height: extrasRow.implicitHeight
            clip: true
            opacity: toolbar._shownVideo ? 1 : 0

            Behavior on width {
                enabled: !toolbar._snap
                NumberAnimation {
                    duration: Tokens.durationPanel
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Tokens.easing
                }
            }
            Behavior on opacity {
                enabled: !toolbar._snap
                NumberAnimation {
                    duration: Tokens.durationFade
                }
            }

            Row {
                id: extrasRow
                x: Tokens.barGap
                spacing: Tokens.barGap

                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: Tokens.dividerWidth
                    height: Tokens.dividerHeight
                    color: Tokens.outline
                }

                CaptureButton {
                    anchors.verticalCenter: parent.verticalCenter
                    icon: Capture.ICON_DESKTOP_AUDIO
                    label: qsTr("Desktop")
                    radius: height / 2
                    selected: toolbar.desktopAudio
                    onClicked: toolbar.desktopAudioToggled()
                }

                CaptureButton {
                    anchors.verticalCenter: parent.verticalCenter
                    icon: Capture.ICON_MIC
                    label: qsTr("Mic")
                    radius: height / 2
                    selected: toolbar.micAudio
                    onClicked: toolbar.micAudioToggled()
                }
            }
        }

        Row {
            id: tail
            anchors.verticalCenter: parent.verticalCenter
            x: extras.x + extras.width + Tokens.barGap
            spacing: Tokens.barGap

            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: Tokens.dividerWidth
                height: Tokens.dividerHeight
                color: Tokens.outline
            }

            CaptureButton {
                anchors.verticalCenter: parent.verticalCenter
                icon: Capture.ICON_CLOSE
                radius: height / 2
                onClicked: toolbar.closeRequested()
            }
        }
    }
}
