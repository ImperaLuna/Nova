.pragma library
.import "../daemon/protocol.js" as Protocol
.import "../text/bytes.js" as Bytes
.import "../theme/icons.js" as Icons
.import "paths.js" as Paths

// Nova Files' Games place as its window shows it, kept apart so ui/tests
// can check it: a game as a row, and what its menu and the command row
// offer.

// The games' path, a place rather than a folder.
var PLACE = Protocol.FILES_GAMES_PLACE;

// The place's name, in the sidebar, the address bar and the window title.
function name() {
    return qsTr("Games");
}

// Whether a window at `folder` shows the games.
function isGames(folder) {
    return folder === PLACE;
}

// What a row holds: a game ({name, folder, size, saves}) with the size -1
// and the saves "" when novad found none, so every row has every role.
function row(game) {
    return {
        name: game.name,
        folder: game.folder,
        size: game.size ?? -1,
        saves: game.saves ?? ""
    };
}

// The Size column: empty when the launcher doesn't say.
function size(row) {
    return row.size < 0 ? "" : Bytes.bytes(row.size);
}

// What a game's menu and the command row offer for `row` (null: none
// picked), each {icon, label, folder}: the folder to go to, "" when
// there's none.
function actions(row) {
    return [
        {
            icon: Icons.FOLDER,
            label: qsTr("Open install folder"),
            folder: row?.folder ?? ""
        },
        {
            icon: Icons.SAVE,
            label: qsTr("Open save folder"),
            folder: row?.saves ?? ""
        }
    ];
}
