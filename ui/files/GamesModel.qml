import QtQuick
import "games.js" as Games

// The Games place as a window shows it: `rows` holds every installed game
// (files_games_list) in name order, read each time the window goes there
// and when novad comes back. `picked` is the one game picked, by its
// folder, so it stays on the same game when the list is read again. Only
// read while `active`, so a window that never shows the games never asks.
// It reads through `service` (FilesService, given by the window), so tests
// can make one without novad.
QtObject {
    id: games

    // What it asks novad through: gamesList(done).
    property var service: null
    property bool active: false
    readonly property ListModel rows: ListModel {}
    property bool loaded: false
    // The picked game's row index, or -1.
    property int picked: -1
    // The picked game's row, or null.
    readonly property var pickedRow: picked >= 0 && picked < rows.count ? _items[picked] : null

    // The rows as plain objects, in the list's order.
    property var _items: []
    // Which read the rows show, so a late answer to an earlier one is
    // dropped.
    property int _reading: 0

    function pick(index: int) {
        picked = index >= 0 && index < rows.count ? index : -1;
    }

    function refresh() {
        if (!active || !service)
            return;
        const reading = ++_reading;
        service.gamesList(found => {
            if (reading !== games._reading)
                return;
            // A failed read keeps what's shown.
            if (found !== null)
                games._fill(found);
        });
    }

    function _fill(found: var) {
        const kept = pickedRow?.folder ?? "";
        const shown = found.map(game => Games.row(game));
        picked = -1;
        rows.clear();
        for (const row of shown)
            rows.append(row);
        _items = shown;
        picked = shown.findIndex(row => row.folder === kept);
        loaded = true;
    }

    onActiveChanged: {
        if (active)
            refresh();
        else
            picked = -1;
    }
    // A window that opens at the games is active from the start.
    Component.onCompleted: refresh()

    readonly property Connections _novad: Connections {
        target: games.service
        function onConnectedChanged() {
            if (games.service.connected)
                games.refresh();
        }
    }
}
