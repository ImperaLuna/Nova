import QtQuick
import "../theme"

// What the lock screen shows: the user's initial in a circle, their name,
// the password field (a dot per character typed, a spinner while it's
// checked) and a line for what went wrong. Only draws: the keys and PAM are
// LockScreen.qml's.
Column {
    id: root

    required property string userName
    // Characters typed so far; their text never reaches this item.
    required property int typed
    required property bool checking
    // The last try was wrong; the field rings in the error color.
    required property bool failed
    required property string message

    // Shakes the field, after a wrong password.
    function shake() {
        shaking.restart();
    }

    spacing: Tokens.lockGap

    Rectangle {
        anchors.horizontalCenter: parent.horizontalCenter
        width: Tokens.lockAvatarSize
        height: Tokens.lockAvatarSize
        radius: width / 2
        color: Tokens.surfaceRaised
        border.color: Tokens.outline
        border.width: Tokens.strokeThin

        Text {
            anchors.centerIn: parent
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.lockInitialSize
            font.weight: Tokens.weightLabel
            color: Tokens.fgDim
            textFormat: Text.PlainText
            text: root.userName.charAt(0).toUpperCase()
        }
    }

    Text {
        anchors.horizontalCenter: parent.horizontalCenter
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeTitle
        font.weight: Tokens.weightTitle
        color: Tokens.fg
        textFormat: Text.PlainText
        text: root.userName
    }

    Item {
        anchors.horizontalCenter: parent.horizontalCenter
        width: Tokens.lockFieldWidth
        height: Tokens.lockFieldHeight

        Rectangle {
            id: field
            objectName: "field"
            // 0 to 1 through a shake: back and forth, each swing smaller,
            // ending where it started.
            property real shake: 0
            x: Tokens.lockShake * Math.sin(shake * Math.PI * Tokens.lockShakeSwings) * (1 - shake)
            width: parent.width
            height: parent.height
            radius: Tokens.radiusControl
            color: Tokens.surfaceRaised
            border.width: Tokens.strokeThin
            border.color: root.failed ? Tokens.error : Tokens.alpha(Tokens.primary, Tokens.lockFocusOpacity)

            Behavior on border.color {
                ColorAnimation {
                    duration: Tokens.durationColor
                }
            }

            Row {
                id: dots
                anchors.verticalCenter: parent.verticalCenter
                x: Tokens.pad
                spacing: Tokens.lockDotGap
                // As many as fit before the spinner's room.
                readonly property int fits: Math.max(0, Math.floor((field.width - 3 * Tokens.pad - Tokens.iconSize) / (Tokens.lockDotSize + Tokens.lockDotGap)))

                Repeater {
                    model: Math.min(root.typed, dots.fits)

                    Rectangle {
                        width: Tokens.lockDotSize
                        height: Tokens.lockDotSize
                        radius: width / 2
                        color: Tokens.fg
                    }
                }
            }

            Text {
                anchors.verticalCenter: parent.verticalCenter
                x: Tokens.pad
                visible: root.typed === 0
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                color: Tokens.fgDim
                text: qsTr("Password")
            }

            Spinner {
                anchors.verticalCenter: parent.verticalCenter
                anchors.right: parent.right
                anchors.rightMargin: Tokens.pad
                visible: root.checking
            }

            NumberAnimation {
                id: shaking
                target: field
                property: "shake"
                from: 0
                to: 1
                duration: Tokens.durationShake
            }
        }
    }

    Text {
        anchors.horizontalCenter: parent.horizontalCenter
        height: Tokens.fontSizeSmall * Tokens.lineHeightWrapped
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeSmall
        color: root.failed ? Tokens.error : Tokens.fgDim
        textFormat: Text.PlainText
        text: root.message
    }
}
