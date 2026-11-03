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

    // Takes a key press (its `text` and `modifiers`) as typing among the
    // rows of `rows` (a ListModel with a `name` role) from row `current`,
    // calling `go(index)` with the row it finds. False when the press isn't
    // typing; the names are read only when it is.
    function press(pressed: string, modifiers: int, rows: ListModel, current: int, go: var): bool {
        const letter = Shortcuts.typed(pressed, modifiers, text);
        if (letter === "")
            return false;
        text += letter;
        _pause.restart();
        const names = [];
        for (let index = 0; index < rows.count; index++)
            names.push(rows.get(index).name);
        const found = Shortcuts.find(names, current, text);
        if (found >= 0)
            go(found);
        return true;
    }
}
