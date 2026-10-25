import QtQuick
import Quickshell
import Quickshell.Wayland
import "../compositor"
import "../theme"

// One screen of the lock screen: the backdrop, and the prompt on the focused
// screen (the first one when novad doesn't know which). Keys go to the
// Authenticator, whichever screen gets them.
WlSessionLockSurface {
    id: surface

    required property Authenticator authenticator

    // Untyped: qmllint doesn't know this `screen` is a ShellScreen.
    readonly property var shownOn: screen
    readonly property bool prompting: {
        const focused = Compositor.outputs.find(output => output.focused);
        return focused ? focused.name === shownOn?.name : shownOn === Quickshell.screens[0];
    }

    color: Tokens.backdrop

    Item {
        anchors.fill: parent
        focus: true
        Keys.onPressed: event => {
            surface.authenticator.key(event);
            event.accepted = true;
        }

        LockPrompt {
            id: prompt
            anchors.centerIn: parent
            visible: surface.prompting
            opacity: surface.authenticator.unlocking ? 0 : 1
            userName: surface.authenticator.userName
            typed: surface.authenticator.password.length
            checking: surface.authenticator.checking
            failed: surface.authenticator.failed
            message: surface.authenticator.message

            Behavior on opacity {
                NumberAnimation {
                    duration: Tokens.durationUnlock
                }
            }

            Connections {
                target: surface.authenticator
                function onRejected() {
                    prompt.shake();
                }
            }
        }
    }
}
