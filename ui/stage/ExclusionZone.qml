import Quickshell

// The space the bar keeps free at the top of a screen, so windows sit below
// it. An empty surface of its own: the stage reserves nothing, so panels
// opening on it never move windows, and this only changes when the bar
// itself shows or hides.
PanelWindow { // qmllint disable uncreatable-type
    required property int size

    anchors {
        top: true
        left: true
        right: true
    }
    exclusiveZone: size
    // A layer surface needs some size; nothing is drawn and it takes no input.
    implicitHeight: 1
    color: "transparent"
    mask: Region {}
}
