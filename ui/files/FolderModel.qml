import QtQuick
import "service"
import "../daemon/protocol.js" as Protocol
import "listing.js" as Listing

// One folder as novad's folder view keeps it (files_open): `rows` holds
// its entries in the view's order, filled page by page and kept current
// from novad's changes, and opened again whenever `path`, the order or
// `hidden` change, or novad comes back. The selection lives here too, by
// name, so it stays on the same entries as rows come and go or the order
// changes; `revision` moves on each change of it, for delegates to bind to.
QtObject {
    id: folder

    required property string path
    // {key, descending}: key is a Protocol.FILE_SORT_*.
    property var sort: ({
            key: Protocol.FILE_SORT_NAME,
            descending: false
        })
    property bool hidden: false

    readonly property ListModel rows: ListModel {}
    // How many entries the folder holds, once every page is in.
    property int total: 0
    // The first page is in.
    property bool loaded: false
    // Why novad closed the view (the folder couldn't be read or went), or
    // empty while it's open.
    property string error: ""

    property int revision: 0
    property int selectedCount: 0
    // The row Shift selects from: the last plain or Ctrl click's.
    property int anchor: -1

    // The rows were filled anew; `current` is the first selected row's
    // index (or -1), for the list to move to.
    signal reset(int current)

    // This view's number, handed out as it's made.
    property int view: 0
    property var _selected: ({})
    property bool _wanted: false

    function isSelected(name: string): bool {
        return _selected[name] === true;
    }

    // Selects the entries named `names` (in this folder) and nothing else.
    function selectNames(names: var) {
        _selected = {};
        for (const name of names)
            _selected[name] = true;
        selectedCount = names.length;
        revision++;
    }

    // Applies a Listing.pick plan.
    function select(plan: var) {
        if (plan.clear) {
            _selected = {};
            selectedCount = 0;
        }
        for (let index = plan.from; index <= plan.to; index++) {
            const name = rows.get(index).name;
            const on = plan.toggle ? !isSelected(name) : true;
            if (on !== isSelected(name))
                selectedCount += on ? 1 : -1;
            if (on)
                _selected[name] = true;
            else
                delete _selected[name];
        }
        anchor = plan.anchor;
        revision++;
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

    // The selected rows, in the list's order.
    function selectedRows(): var {
        const picked = [];
        for (let index = 0; index < rows.count && picked.length < selectedCount; index++) {
            const row = rows.get(index);
            if (isSelected(row.name))
                picked.push(row);
        }
        return picked;
    }

    function _open() {
        if (!_wanted || !FilesService.connected)
            return;
        error = "";
        FilesService.open(view, path, sort.key, sort.descending, hidden, failure => {
            if (failure)
                error = failure;
        });
    }

    function _page(page: var) {
        if (view === 0 || page.view !== view || page.path !== path)
            return;
        if (page.start === 0) {
            rows.clear();
            anchor = -1;
        } else if (page.start < rows.count) {
            rows.remove(page.start, rows.count - page.start);
        }
        rows.append(page.entries.map(Listing.row));
        total = page.total;
        let current = -1;
        if (page.start === 0) {
            for (let index = 0; index < rows.count && current < 0; index++) {
                if (isSelected(rows.get(index).name))
                    current = index;
            }
            loaded = true;
            reset(current);
        }
    }

    function _changed(changed: var) {
        if (changed.view !== view)
            return;
        for (const change of changed.changes) {
            if (change.op === Protocol.FOLDER_INSERT) {
                rows.insert(change.index, Listing.row(change.entry));
                total++;
            } else if (change.op === Protocol.FOLDER_REMOVE) {
                const name = rows.get(change.index).name;
                if (isSelected(name)) {
                    delete _selected[name];
                    selectedCount--;
                    revision++;
                }
                rows.remove(change.index);
                total--;
            } else if (change.op === Protocol.FOLDER_UPDATE) {
                rows.set(change.index, Listing.row(change.entry));
            }
            anchor = Math.min(Listing.shifted(anchor, change), rows.count - 1);
        }
    }

    function _closed(closed: var) {
        if (closed.view !== view)
            return;
        rows.clear();
        total = 0;
        error = closed.error;
    }

    onPathChanged: {
        loaded = false;
        selectNames([]);
        _open();
    }
    onSortChanged: _open()
    onHiddenChanged: _open()

    Component.onCompleted: {
        view = FilesService.newView();
        _wanted = true;
        _open();
    }
    Component.onDestruction: {
        _wanted = false;
        if (FilesService.connected)
            FilesService.close(view);
    }

    readonly property Connections _service: Connections {
        target: FilesService
        function onPage(page: var) {
            folder._page(page);
        }
        function onChanged(changed: var) {
            folder._changed(changed);
        }
        function onViewClosed(closed: var) {
            folder._closed(closed);
        }
        function onConnectedChanged() {
            folder._open();
        }
    }
}
