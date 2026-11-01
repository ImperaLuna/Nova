import QtQuick
import Quickshell
import Quickshell.Wayland
import "../compositor"
import "../stage"
import "../theme"
import "../theme/icons.js" as Icons
import "service"
import "bluetooth.js" as Bluetooth

// A pairing question while the Bluetooth page isn't there to ask it: a
// phone pairing with this computer, or a device you started pairing before
// closing quick settings. Its own layer surface under the right pill of
// the screen that had focus when the question came, on the overlay layer
// like the notifications' pop-ups, taking no space, and the keyboard only
// when clicked (a PIN to type). It goes once the question is answered or
// over.
PanelWindow { // qmllint disable uncreatable-type
    id: window

    readonly property CoversGlass cover: CoversGlass {
        shown: window.visible
    }

    readonly property var request: BluetoothService.watching ? null : BluetoothService.pairing
    // The screen it shows on, picked when a question comes.
    property ShellScreen target: null
    readonly property bool asking: request !== null
    onAskingChanged: {
        if (asking)
            target = Compositor.focusedScreen();
    }

    screen: target
    visible: request !== null && target !== null
    anchors.top: true
    anchors.right: true
    readonly property real top: Tokens.barGap + Tokens.barHeight + Tokens.gap
    implicitWidth: Tokens.notificationWidth + Tokens.barMargin + Tokens.shadowRoom
    implicitHeight: Math.ceil(top + card.height + Tokens.shadowRoom)
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "nova-bluetooth"
    WlrLayershell.keyboardFocus: request && Bluetooth.typed(request.ask) ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None

    mask: Region {
        item: card
        radius: Tokens.radiusNotification
    }
    Region {
        id: blur
        item: card
        radius: Tokens.radiusNotification
    }
    Component.onCompleted: {
        BackgroundEffect.blurRegion = blur;
        if (asking)
            target = Compositor.focusedScreen();
    }

    Item {
        id: card
        x: Tokens.shadowRoom
        y: window.top
        width: Tokens.notificationWidth
        height: content.implicitHeight + 2 * Tokens.pad

        GlassShadow {
            glass: surface
        }
        Glass {
            id: surface
            anchors.fill: parent
            radius: Tokens.radiusNotification
        }

        Column {
            id: content
            x: Tokens.pad
            y: Tokens.pad
            width: parent.width - 2 * Tokens.pad
            spacing: Tokens.gap

            LineIcon {
                path: Icons.BLUETOOTH
                size: Tokens.iconSizeSmall
                color: Tokens.primary
            }

            PairingPrompt {
                width: parent.width
                visible: window.request !== null
                request: window.request ?? Bluetooth.NO_REQUEST
                name: Bluetooth.deviceName(BluetoothService.devices, request.address)
                onAnswered: answer => BluetoothService.answer(request.id, answer)
            }
        }
    }

    FrameLog {
        surfaceName: `${window.target?.name ?? ""}/bluetooth`
    }
}
