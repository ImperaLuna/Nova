import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// The sidebar's top: the distribution's logo in a circle, a greeting with
// the user's name, and the host with its uptime under it.
Item {
    id: header

    // The logo's file, or "" for Nova's own mark.
    property string logo: ""
    property string greeting: ""
    property string name: ""
    property string detail: ""

    implicitHeight: Tokens.logoSize

    Rectangle {
        id: badge
        anchors.verticalCenter: parent.verticalCenter
        width: Tokens.logoSize
        height: Tokens.logoSize
        radius: width / 2
        color: Tokens.surfaceRaised

        Image {
            anchors.centerIn: parent
            visible: header.logo !== "" && status === Image.Ready
            width: Tokens.logoMark
            height: Tokens.logoMark
            sourceSize: Qt.size(Tokens.logoMark, Tokens.logoMark)
            fillMode: Image.PreserveAspectFit
            source: header.logo
            asynchronous: true
        }
        LineIcon {
            anchors.centerIn: parent
            visible: header.logo === ""
            path: Icons.STAR
            size: Tokens.iconSizeMenu
            color: Tokens.primary
        }
    }

    Column {
        anchors.verticalCenter: parent.verticalCenter
        x: badge.width + Tokens.pad
        width: header.width - x

        Row {
            width: parent.width
            clip: true

            Text {
                font.family: Tokens.fontDisplay
                font.pixelSize: Tokens.fontSizeGreeting
                color: Tokens.fg
                text: header.name === "" ? header.greeting : qsTr("%1, ").arg(header.greeting)
            }
            Text {
                font.family: Tokens.fontDisplay
                font.pixelSize: Tokens.fontSizeGreeting
                font.italic: true
                color: Tokens.primary
                textFormat: Text.PlainText
                text: header.name
            }
        }

        Row {
            spacing: Tokens.iconGap
            visible: header.detail !== ""

            LineIcon {
                anchors.verticalCenter: parent.verticalCenter
                path: Icons.CLOCK
                size: Tokens.iconSizeSmall
                color: Tokens.fgDim
            }
            Text {
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeTiny
                color: Tokens.fgDim
                textFormat: Text.PlainText
                text: header.detail
            }
        }
    }
}
