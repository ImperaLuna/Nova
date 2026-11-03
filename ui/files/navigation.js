.pragma library

// Where a Nova Files window has been and what its sidebar tree keeps open,
// as plain values the window replaces on each move, so ui/tests can check
// them. novad works out the tree itself (files_tree); this only remembers
// what the user asked of it.

// A history holding only `folder`.
function start(folder) {
    return {
        entries: [folder],
        at: 0
    };
}

// `history` after going to `folder`: what was ahead of the current place
// is dropped, as in a browser. Going where the window already is changes
// nothing.
function visit(history, folder) {
    if (history.entries[history.at] === folder)
        return history;
    return {
        entries: history.entries.slice(0, history.at + 1).concat([folder]),
        at: history.at + 1
    };
}

// `history` moved `steps` back (negative) or forward, or null when there's
// nothing that way.
function step(history, steps) {
    const at = history.at + steps;
    if (at < 0 || at >= history.entries.length)
        return null;
    return {
        entries: history.entries,
        at: at
    };
}

function current(history) {
    return history.entries[history.at];
}

// The tree's kept folders after the user clicks the chevron of the row at
// `path`, which is `open` now: {opened, closed}, the folders the user
// opened and the ones on the way to the window's folder they closed. Each
// list keeps its newest `most` folders.
function toggle(kept, path, open, most) {
    const without = list => list.filter(other => other !== path);
    const opened = open ? without(kept.opened) : without(kept.opened).concat([path]);
    const closed = open ? without(kept.closed).concat([path]) : without(kept.closed);
    return {
        opened: opened.slice(-most),
        closed: closed.slice(-most)
    };
}

// Makes `model` (a ListModel of tree rows) hold `rows` (from files_tree)
// in order, removing and inserting only the rows that come or go, so the
// rest stay put and only new ones animate in. A kept row that changed
// place is moved, never added twice.
function sync(model, rows) {
    const wanted = {};
    for (const row of rows)
        wanted[row.path] = true;
    for (let i = model.count - 1; i >= 0; i--) {
        if (!wanted[model.get(i).path])
            model.remove(i);
    }
    for (let i = 0; i < rows.length; i++) {
        let at = -1;
        for (let k = i; k < model.count; k++) {
            if (model.get(k).path === rows[i].path) {
                at = k;
                break;
            }
        }
        if (at === -1) {
            model.insert(i, rows[i]);
            continue;
        }
        if (at !== i)
            model.move(at, i, 1);
        model.set(i, rows[i]);
    }
}
