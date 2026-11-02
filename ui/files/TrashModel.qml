import QtQuick
import "trash.js" as Trash

// The trash as a window shows it: `rows` holds every trashed item
// (files_trash_list), the last trashed first, read again whenever novad
// says the trash changed, in Nova Files or another app. The selection
// lives here, by id, so it stays on the same items as others come and go;
// `revision` moves on each change of it, for delegates to bind to. Only
// read while `active`, so a window that never shows the trash never asks.
// It reads through `service` (FilesService, given by the window), so tests
// can make one without novad.
QtObject {
    id: trash

    // What it asks novad through: trashList(done) and a trashChanged
    // signal.
    property var service: null
    property bool active: false
    readonly property ListModel rows: ListModel {}
    property bool loaded: false

    property int revision: 0
    property int selectedCount: 0
    // The row Shift selects from: the last plain or Ctrl click's.
    property int anchor: -1

    // Selected ids as keys of a map without a prototype.
    property var _selected: Object.create(null)
    // The anchor's item, so Shift keeps selecting from it as rows move.
    property string _anchorId: ""
    // The rows as plain objects, in the list's order: what selectedRows and
    // allRows hand out, which stay as they are when `rows` is read again.
    property var _items: []
    // Which read the rows show, so a late answer to an earlier one is
    // dropped.
    property int _reading: 0

    function isSelected(id: string): bool {
        return _selected[id] === true;
    }

    // Applies a Listing.pick plan.
    function select(plan: var) {
        if (plan.clear)
            _selected = Object.create(null);
        for (let index = plan.from; index <= plan.to; index++) {
            const id = rows.get(index).trashId;
            if (plan.toggle && isSelected(id))
                delete _selected[id];
            else
                _selected[id] = true;
        }
        anchor = plan.anchor;
        _anchorId = anchor >= 0 && anchor < rows.count ? rows.get(anchor).trashId : "";
        _changed();
    }

    function selectAll() {
        select({
            clear: true,
            from: 0,
            to: rows.count - 1,
            toggle: false,
            anchor: anchor
        });
    }

    function clearSelection() {
        _selected = Object.create(null);
        anchor = -1;
        _anchorId = "";
        _changed();
    }

    // The selected rows, in the list's order.
    function selectedRows(): var {
        return _items.filter(row => isSelected(row.trashId));
    }

    // Every row, in the list's order.
    function allRows(): var {
        return _items.slice();
    }

    function refresh() {
        if (!active || !service)
            return;
        const reading = ++_reading;
        service.trashList(items => {
            if (reading !== trash._reading)
                return;
            // A failed read keeps what's shown.
            if (items !== null)
                trash._fill(items);
        });
    }

    function _fill(items: var) {
        const kept = Object.create(null);
        const shown = items.map(item => Trash.row(item));
        rows.clear();
        let found = -1;
        for (const row of shown) {
            if (row.trashId === _anchorId)
                found = rows.count;
            rows.append(row);
            if (isSelected(row.trashId))
                kept[row.trashId] = true;
        }
        _items = shown;
        _selected = kept;
        anchor = found;
        if (found < 0)
            _anchorId = "";
        loaded = true;
        _changed();
    }

    function _changed() {
        selectedCount = Object.keys(_selected).length;
        revision++;
    }

    onActiveChanged: {
        if (active)
            refresh();
        else
            clearSelection();
    }

    readonly property Connections _novad: Connections {
        target: trash.service
        function onTrashChanged() {
            trash.refresh();
        }
    }
}
