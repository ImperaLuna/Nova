import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// The capsule's icon: a solid speaker with no, one or two waves ("off",
// "low", "high"), crossed out ("muted"), struck through ("none", no
// output), headphones with solid cups, the Bluetooth rune or a monitor
// ("display", HDMI or DisplayPort audio).
LineIcon {
    // volume.js icon().
    required property string glyph

    path: ({
            off: "",
            low: Icons.WAVE,
            high: Icons.WAVES,
            muted: Icons.MUTED,
            none: Icons.STRIKE,
            headphones: Icons.HEADPHONES_BAND,
            bluetooth: Icons.BLUETOOTH,
            display: Icons.MONITOR
        })[glyph] ?? ""
    fill: ({
            headphones: Icons.HEADPHONES_CUPS,
            bluetooth: "",
            display: ""
        })[glyph] ?? Icons.SPEAKER_BODY
}
