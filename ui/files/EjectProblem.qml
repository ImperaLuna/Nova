import QtQuick
import "../theme"

// Windows' "Problem Ejecting" dialog, in Nova's look: the drive's name,
// Windows' one line, and OK. Shown in the middle of the window while a
// drive couldn't be ejected because something still uses it; the drive
// stays mounted.
Item {
    id: problem

    property string name: ""

    function show(name: string) {
        problem.name = name;
    }

    visible: name !== ""

    // Clicks don't reach the window behind it.
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.AllButtons
    }

    Rectangle {
        anchors.centerIn: parent
        width: Tokens.filesDialogWidth
        height: body.implicitHeight + 2 * Tokens.filesDialogPad
        radius: Tokens.radiusPanel
        color: Tokens.bg
        border.width: Tokens.dividerWidth
        border.color: Tokens.glassRim
        scale: problem.visible ? 1 : Tokens.enterScale

        Behavior on scale {
            NumberAnimation {
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }

        Column {
            id: body
            anchors.fill: parent
            anchors.margins: Tokens.filesDialogPad
            spacing: Tokens.pad

            Text {
                width: parent.width
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeTitle
                font.weight: Tokens.weightTitle
                color: Tokens.fg
                wrapMode: Text.Wrap
                textFormat: Text.PlainText
                text: qsTr("Problem Ejecting %1").arg(problem.name)
            }
            Text {
                width: parent.width
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                color: Tokens.filesFaint
                wrapMode: Text.Wrap
                lineHeight: Tokens.lineHeightWrapped
                text: qsTr("This device is currently in use. Close any programs or windows that might be using the device, and then try again.")
            }
            Item {
                width: parent.width
                height: Tokens.filesFieldHeight

                Rectangle {
                    anchors.right: parent.right
                    width: okLabel.implicitWidth + 2 * Tokens.filesDialogButtonPad
                    height: parent.height
                    radius: height / 2
                    color: Tokens.primary

                    Text {
                        id: okLabel
                        anchors.centerIn: parent
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        font.weight: Tokens.weightTitle
                        color: Tokens.onAccent
                        text: qsTr("OK")
                    }
                    TapHandler {
                        onTapped: problem.name = ""
                    }
                }
            }
        }
    }

    Keys.onReturnPressed: name = ""
    Keys.onEscapePressed: name = ""
    onVisibleChanged: if (visible)
        forceActiveFocus()
}
