import QtQuick
import "../theme"
import "shortcuts.js" as Shortcuts

// Type-to-select for a file list, as in Explorer: letters typed while the
// list has the keyboard spell a name, and the list goes to the first row
// starting with it (Shortcuts.find). A pause of Tokens.filesTypingPause
// starts a new name.
QtObject {
    id: typing

    // The name typed so far.
    property string text: ""

    readonly property Timer _pause: Timer {
        interval: Tokens.filesTypingPause
        onTriggered: typing.text = ""
    }

    // Takes a key press (its `text` and `modifiers`) as typing among
    // `names` from row `current`, calling `go(index)` with the row it
    // finds. False when the press isn't typing.
    function press(pressed: string, modifiers: int, names: var, current: int, go: var): bool {
        const letter = Shortcuts.typed(pressed, modifiers, text);
        if (letter === "")
            return false;
        text += letter;
        _pause.restart();
        const found = Shortcuts.find(names, current, text);
        if (found >= 0)
            go(found);
        return true;
    }
}
