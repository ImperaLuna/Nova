import QtQuick
import "service"
import "trash.js" as Trash

// The trash as a window shows it: `rows` holds every trashed item
// (files_trash_list), the last trashed first, read again whenever novad
// says the trash changed, in Nova Files or another app. The selection
// lives here, by id, so it stays on the same items as others come and go;
// `revision` moves on each change of it, for delegates to bind to. Only
// read while `active`, so a window that never shows the trash never asks.
QtObject {
    id: trash

    property bool active: false
    readonly property ListModel rows: ListModel {}
    property bool loaded: false

    property int revision: 0
    property int selectedCount: 0
    // The row Shift selects from: the last plain or Ctrl click's.
    property int anchor: -1

    // Selected ids as keys of a map without a prototype.
    property var _selected: Object.create(null)
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
        _changed();
    }

    // The selected rows, in the list's order.
    function selectedRows(): var {
        const picked = [];
        for (let index = 0; index < rows.count; index++) {
            const row = rows.get(index);
            if (isSelected(row.trashId))
                picked.push(row);
        }
        return picked;
    }

    // Every row, in the list's order.
    function allRows(): var {
        const all = [];
        for (let index = 0; index < rows.count; index++)
            all.push(rows.get(index));
        return all;
    }

    function refresh() {
        if (!active)
            return;
        const reading = ++_reading;
        FilesService.trashList(items => {
            if (reading !== trash._reading)
                return;
            trash._fill(items);
        });
    }

    function _fill(items: var) {
        const kept = Object.create(null);
        rows.clear();
        for (const item of items) {
            rows.append(Trash.row(item));
            if (isSelected(item.id))
                kept[item.id] = true;
        }
        _selected = kept;
        if (anchor >= rows.count)
            anchor = -1;
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
        target: FilesService
        function onTrashChanged() {
            trash.refresh();
        }
    }
}
