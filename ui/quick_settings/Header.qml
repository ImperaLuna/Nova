import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// The sidebar's top: the distribution's logo (no backdrop), a greeting with
// the user's name, and the host with its uptime under it.
Item {
    id: header

    // The logo's file, or "" for Nova's own mark.
    property string logo: ""
    property string greeting: ""
    property string name: ""
    property string detail: ""

    implicitHeight: Tokens.logoSize

    Item {
        id: badge
        anchors.verticalCenter: parent.verticalCenter
        width: Tokens.logoSize
        height: Tokens.logoSize

        Image {
            anchors.fill: parent
            visible: header.logo !== "" && status === Image.Ready
            sourceSize: Qt.size(Tokens.logoSize, Tokens.logoSize)
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

        // The greeting at its size where it fits, else smaller, down to
        // fontSizeGreetingMin; then the name is cut short.
        TextMetrics {
            id: hello
            font.family: Tokens.fontDisplay
            font.pixelSize: Tokens.fontSizeGreeting
            text: greeting.text
        }
        TextMetrics {
            id: called
            font.family: Tokens.fontDisplay
            font.pixelSize: Tokens.fontSizeGreeting
            font.italic: true
            text: header.name
        }

        Row {
            id: greetingRow
            width: parent.width
            readonly property real wide: hello.advanceWidth + called.advanceWidth
            readonly property int size: wide <= width ? Tokens.fontSizeGreeting : Math.max(Tokens.fontSizeGreetingMin, Math.floor(Tokens.fontSizeGreeting * width / wide))

            Text {
                id: greeting
                objectName: "greeting"
                font.family: Tokens.fontDisplay
                font.pixelSize: greetingRow.size
                color: Tokens.fg
                text: header.name === "" ? header.greeting : qsTr("%1, ").arg(header.greeting)
            }
            Text {
                objectName: "name"
                width: Math.min(implicitWidth, greetingRow.width - greeting.width)
                elide: Text.ElideRight
                font.family: Tokens.fontDisplay
                font.pixelSize: greetingRow.size
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
