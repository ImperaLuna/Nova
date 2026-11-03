pragma Singleton

import QtQuick
import Quickshell
import "../clipboard.js" as Clipboard
import "../../daemon/protocol.js" as Protocol

// Cut, copy and paste for every Nova Files window: copied items go on the
// system clipboard as file URIs in text (clipboard.js), so other apps
// paste their names, and Nova Files pastes such text back. A cut is
// kept here too, since text can't say "move": pasting the same text moves
// the items, and empties the clipboard after, as in Explorer. Cut items
// read faded in the windows (`cut`, absolute paths) until then.
Singleton {
    id: clipboard

    // The items last cut, while the clipboard still holds them.
    readonly property var cut: _cutText !== "" && Quickshell.clipboardText === _cutText ? _cutPaths : []
    // A paste would bring something: the clipboard names files.
    readonly property bool hasFiles: Clipboard.paths(Quickshell.clipboardText).length > 0

    property string _cutText: ""
    property var _cutPaths: []

    // Anything else put on the clipboard ends the cut, so the same text
    // copied again later copies.
    readonly property Connections _watch: Connections {
        target: Quickshell
        function onClipboardTextChanged() {
            if (Quickshell.clipboardText !== clipboard._cutText) {
                clipboard._cutText = "";
                clipboard._cutPaths = [];
            }
        }
    }

    // Puts the items at `paths` on the clipboard; `move` for a cut.
    function put(paths: var, move: bool) {
        const text = Clipboard.text(paths);
        _cutText = move ? text : "";
        _cutPaths = move ? paths : [];
        Quickshell.clipboardText = text;
    }

    // Puts plain `text` on the clipboard: Copy path.
    function putText(text: string) {
        _cutText = "";
        _cutPaths = [];
        Quickshell.clipboardText = text;
    }

    // What a paste does now: {mode (a Protocol.TRANSFER_* mode), paths}.
    // Taking a cut empties the clipboard, so it's moved once.
    function take(): var {
        const text = Quickshell.clipboardText;
        const paths = Clipboard.paths(text);
        const move = _cutText !== "" && text === _cutText;
        if (move)
            putText("");
        return {
            mode: move ? Protocol.TRANSFER_MOVE : Protocol.TRANSFER_COPY,
            paths: paths
        };
    }
}
