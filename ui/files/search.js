.pragma library
.import "listing.js" as Listing
.import "paths.js" as Paths

// Nova Files' search logic, kept apart so ui/tests can check it: how a
// file novad found (files_search's FoundFile) reads as a row of the
// results (its name with the matched words marked, where it lives, dimmer
// when it's hidden), and the count under them.

// A result as a row for a ListModel: Listing.row's roles, plus `folder`
// (absolute, wire text), `dim` (it or a folder it's in is hidden) and
// `marks` (where the query's words are in its name).
function row(found) {
    const entry = Listing.row(found.entry);
    entry.folder = found.folder;
    entry.dim = found.entry.hidden || found.in_hidden_folder;
    entry.marks = found.marks.map(mark => ({
                start: mark.start,
                length: mark.length
            }));
    return entry;
}

// The absolute path of a result row.
function path(row) {
    return Paths.child(row.folder, row.name);
}

// Where a result lives, for people: from `~` inside `home`, else the
// absolute path.
function location(folder, home) {
    if (home !== "" && Paths.inside(folder, home))
        return Paths.shown("~" + folder.slice(home.length));
    return Paths.shown(folder);
}

// `name` (wire text) as styled text with each of `marks` in bold in
// `color`. Everything else is escaped, so a `<` in a name stays a `<`.
function marked(name, marks, color) {
    let out = "";
    let at = 0;
    const sorted = marks.slice().sort((a, b) => a.start - b.start);
    for (const mark of sorted) {
        const start = Math.max(mark.start, at);
        const end = Math.min(mark.start + mark.length, name.length);
        if (end <= start)
            continue;
        out += escaped(Paths.shown(name.slice(at, start)));
        out += `<b><font color="${color}">${escaped(Paths.shown(name.slice(start, end)))}</font></b>`;
        at = end;
    }
    return out + escaped(Paths.shown(name.slice(at)));
}

function escaped(text) {
    return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

// The line under the results: "Indexing" until novad's index is built,
// then how many were found, and how many of them show when not all do.
function count(ready, total, shown) {
    if (!ready)
        return qsTr("Indexing");
    if (total === 0)
        return qsTr("No results");
    if (shown < total)
        return qsTr("%1 of %2 results").arg(shown).arg(total);
    return total === 1 ? qsTr("1 result") : qsTr("%1 results").arg(total);
}
