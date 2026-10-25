import QtQuick
import "../theme"
import "../stage"

// A section of the sidebar that fades in and settles down as it opens, the
// `order`th one after another, and fades out with the panel as it closes.
Item {
    id: section

    property bool shown: false
    property int order: 0
    readonly property bool running: reveal.running

    implicitHeight: childrenRect.height
    opacity: shown ? 1 : 0
    MorphReveal on opacity {
        id: reveal
        delay: Tokens.revealFirst + section.order * Tokens.revealStagger
        duration: Tokens.durationPanel
        eased: true
    }
    transform: Translate {
        y: (section.opacity - 1) * Tokens.revealRise
    }
}
