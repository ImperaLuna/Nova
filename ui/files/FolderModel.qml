import QtQuick
import "service"
import "../daemon/protocol.js" as Protocol
import "listing.js" as Listing

// One folder as novad's folder view keeps it (files_open): `rows` holds
// its entries in the view's order, filled page by page and kept current
// from novad's changes, and opened again whenever `path`, the order or
// `hidden` change, or novad comes back; no `path` (a page shows) reads
// nothing. The selection lives here too, by name, so it stays on the
// same entries as rows come and go or the order changes; `revision` moves
// on each change of it, for delegates to bind to.
QtObject {
    id: folder

    required property string path
    // {key, descending, mixed}: key is a Protocol.FILE_SORT_*; mixed
    // lists folders among the files rather than first.
    property var sort: ({
            key: Protocol.FILE_SORT_NAME,
            descending: false
        })
    property bool hidden: false

    readonly property ListModel rows: ListModel {}
    // How many entries the folder holds, once every page is in.
    property int total: 0
    // The first row's size (-1 for none): the biggest, biggest first.
    property real firstSize: -1
    // The first page is in.
    property bool loaded: false
    // Why novad closed the view (the folder couldn't be read or went), or
    // empty while it's open.
    property string error: ""

    property int revision: 0
    property int selectedCount: 0
    // The row Shift selects from: the last plain or Ctrl click's.
    property int anchor: -1

    // A folder's entries are coming in anew: `current` is the first
    // selected one's row (or -1). `reset` follows once they're in, for the
    // list to move to it.
    signal filling(int current)
    signal reset(int current)

    // This view's number, handed out as it's made.
    property int view: 0
    // Selected names as keys of a map without a prototype, so a file
    // named like an Object member (`__proto__`) is a name like any other.
    property var _selected: Object.create(null)
    // Ctrl+A was the last selection: entries still to come are selected
    // as they come.
    property bool _selectingAll: false
    // A selected entry wasn't on the first page: the list moves to it when
    // its page comes.
    property bool _seeking: false
    // Which files_open the view shows, so a late answer to an earlier one
    // can't mark this one failed.
    property int _opening: 0
    property bool _wanted: false

    function isSelected(name: string): bool {
        return _selected[name] === true;
    }

    // Selects the entries named `names` (in this folder) and nothing else.
    function selectNames(names: var) {
        _selected = Object.create(null);
        for (const name of names)
            _selected[name] = true;
        selectedCount = Object.keys(_selected).length;
        _selectingAll = false;
        revision++;
    }

    // Applies a Listing.pick plan.
    function select(plan: var) {
        if (plan.clear) {
            _selected = Object.create(null);
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
        _selectingAll = false;
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
        _selectingAll = true;
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
        // No path: the window shows a page, not a folder.
        if (!_wanted || !FilesService.connected || path === "")
            return;
        error = "";
        const opening = ++_opening;
        FilesService.open(view, path, sort, hidden, failure => {
            if (failure && opening === _opening) {
                error = failure;
                loaded = true;
            }
        });
    }

    // Keeps only the selected names the folder lists, once every page is
    // in: dot files hidden again, or names asked for that aren't there.
    function _keepListed() {
        const listed = Object.create(null);
        let count = 0;
        for (let index = 0; index < rows.count; index++) {
            const name = rows.get(index).name;
            if (isSelected(name)) {
                listed[name] = true;
                count++;
            }
        }
        if (count === selectedCount)
            return;
        _selected = listed;
        selectedCount = count;
        revision++;
    }

    function _page(page: var) {
        if (view === 0 || page.view !== view || page.path !== path)
            return;
        const fresh = page.start === 0;
        const found = page.entries.findIndex(entry => isSelected(entry.name));
        if (fresh) {
            filling(found);
            rows.clear();
            anchor = -1;
            error = "";
            _seeking = found < 0 && selectedCount > 0;
        } else if (page.start < rows.count) {
            rows.remove(page.start, rows.count - page.start);
        }
        if (_selectingAll) {
            for (const entry of page.entries)
                _selected[entry.name] = true;
        }
        rows.append(page.entries.map(Listing.row));
        total = page.total;
        _measureFirst();
        if (rows.count >= total)
            _keepListed();
        else if (_selectingAll)
            selectedCount = Object.keys(_selected).length;
        if (fresh) {
            loaded = true;
            reset(found);
        } else if (_seeking && found >= 0) {
            _seeking = false;
            reset(page.start + found);
        }
    }

    // A files_changed event: an entry that moves (removed, then inserted
    // again under the same name, as a new size or time reorders it) stays
    // selected; one that goes for good leaves the selection.
    function _changed(changed: var) {
        if (changed.view !== view)
            return;
        const moving = Object.create(null);
        for (const change of changed.changes) {
            if (change.op === Protocol.FOLDER_INSERT) {
                rows.insert(change.index, Listing.row(change.entry));
                delete moving[change.entry.name];
                total++;
            } else if (change.op === Protocol.FOLDER_REMOVE) {
                const name = rows.get(change.index).name;
                if (isSelected(name))
                    moving[name] = true;
                rows.remove(change.index);
                total--;
            } else if (change.op === Protocol.FOLDER_UPDATE) {
                rows.set(change.index, Listing.row(change.entry));
            }
            anchor = Math.min(Listing.shifted(anchor, change), rows.count - 1);
        }
        _measureFirst();
        const gone = Object.keys(moving);
        if (gone.length === 0)
            return;
        for (const name of gone)
            delete _selected[name];
        selectedCount -= gone.length;
        revision++;
    }

    function _closed(closed: var) {
        if (closed.view !== view)
            return;
        _clear();
        error = closed.error;
        loaded = true;
    }

    function _measureFirst() {
        firstSize = rows.count > 0 ? rows.get(0).size : -1;
    }

    function _clear() {
        rows.clear();
        total = 0;
        firstSize = -1;
        anchor = -1;
        loaded = false;
        selectNames([]);
    }

    // Another folder: its rows go at once, so nothing opens the old ones
    // under the new path while it's read.
    onPathChanged: {
        _clear();
        if (path === "" && view !== 0 && FilesService.connected)
            FilesService.close(view);
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
