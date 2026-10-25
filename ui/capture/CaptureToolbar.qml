pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../daemon/protocol.js" as Protocol
import "capture.js" as Capture

// The region picker's toolbar, where the central pill sits: a screenshot
// or a recording (when both are on), what to take (a region or the whole
// screen), for a recording the sound too (the desktop's, the mic), and
// close. It grows in from the pill's height when
// the picker opens. `glass` is its blur area.
Item {
    id: toolbar

    required property string mode
    // Picking what to record: the sound toggles show.
    property bool video: false
    property bool desktopAudio: false
    property bool micAudio: false
    // Screenshots and recordings are both on and nothing records: the
    // camera and video buttons show. Not while a pick is busy.
    property bool canSwitch: false
    property bool busy: false
    signal videoRequested(bool video)
    signal modeRequested(string mode)
    signal desktopAudioToggled
    signal micAudioToggled
    signal closeRequested

    readonly property Item glass: surface

    implicitWidth: row.implicitWidth + 2 * Tokens.barGap
    implicitHeight: Tokens.captureBarHeight

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
        property bool shown: false
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
            anchors.centerIn: parent
            spacing: Tokens.barGap

            Repeater {
                model: [
                    {
                        video: false,
                        icon: Capture.ICON_CAMERA
                    },
                    {
                        video: true,
                        icon: Capture.ICON_VIDEO
                    }
                ]
                CaptureButton {
                    required property var modelData
                    anchors.verticalCenter: parent.verticalCenter
                    visible: toolbar.canSwitch
                    enabled: !toolbar.busy
                    icon: modelData.icon
                    radius: height / 2
                    selected: toolbar.video === modelData.video
                    onClicked: {
                        if (!selected)
                            toolbar.videoRequested(modelData.video);
                    }
                }
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

            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                visible: toolbar.video
                width: Tokens.dividerWidth
                height: Tokens.dividerHeight
                color: Tokens.outline
            }

            CaptureButton {
                anchors.verticalCenter: parent.verticalCenter
                visible: toolbar.video
                icon: Capture.ICON_DESKTOP_AUDIO
                label: qsTr("Desktop")
                radius: height / 2
                selected: toolbar.desktopAudio
                onClicked: toolbar.desktopAudioToggled()
            }

            CaptureButton {
                anchors.verticalCenter: parent.verticalCenter
                visible: toolbar.video
                icon: Capture.ICON_MIC
                label: qsTr("Mic")
                radius: height / 2
                selected: toolbar.micAudio
                onClicked: toolbar.micAudioToggled()
            }

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
