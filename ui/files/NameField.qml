import QtQuick
import "../theme"
import "listing.js" as Listing

// The field an entry's name turns into while it's renamed (F2, Rename…),
// in a Details row or an Icons tile alike: it opens with the name as novad
// spells it, picked up to its extension as in Explorer, and takes the
// keyboard. Enter or leaving it answers with what's typed, Esc with ""
// (keep the name), once (`done`); no other key reaches the list while it's
// open. Shown only while `renaming`.
Rectangle {
    id: box

    required property string name
    required property bool dir
    required property bool renaming
    // How the typed name sits in the field (TextInput.AlignLeft or
    // AlignHCenter).
    property int alignment: TextInput.AlignLeft

    signal done(string name)

    visible: renaming
    radius: Tokens.filesCrumbRadius
    color: Tokens.filesField
    border.width: Tokens.filesFocusRing
    border.color: Tokens.primary

    TextInput {
        id: field
        anchors.fill: parent
        anchors.leftMargin: Tokens.labelNudge * 4
        anchors.rightMargin: Tokens.labelNudge * 4
        verticalAlignment: TextInput.AlignVCenter
        horizontalAlignment: box.alignment
        clip: true
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSize
        color: Tokens.fg
        selectionColor: Tokens.selected
        selectedTextColor: Tokens.fg
        // Answered once: Enter, Esc or leaving it.
        property bool answered: false

        function finish(typed: string) {
            if (answered)
                return;
            answered = true;
            box.done(typed);
        }

        // After the field's own keys: Enter and Esc answer, and no other
        // key reaches the list while it's open.
        Keys.priority: Keys.AfterItem
        Keys.onPressed: event => {
            if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                finish(text);
            else if (event.key === Qt.Key_Escape)
                finish("");
            event.accepted = true;
        }
        onActiveFocusChanged: {
            if (!activeFocus && box.renaming)
                finish(text);
        }
    }

    onRenamingChanged: {
        if (!renaming)
            return;
        field.answered = false;
        // The name as novad spells it, so bytes that aren't UTF-8 stay as
        // they were in a name that's only partly changed.
        field.text = name;
        field.select(0, Listing.stemLength(field.text, dir));
        field.forceActiveFocus();
    }
    Component.onCompleted: {
        if (renaming)
            renamingChanged();
    }
}
