.pragma library

.import "listing.js" as Listing

// What the details pane says about an item (files_details): its lines of
// facts, and who may do what with it, in Explorer's and COSMIC's words.

// The facts under its name, each {label, value}: Items for a folder, Size
// for a file, then the times the file system keeps, in full.
function facts(details) {
    const row = Listing.row(details.entry);
    const lines = [];
    if (row.dir && details.items !== undefined)
        lines.push({
            label: qsTr("Items"),
            value: String(details.items)
        });
    if (!row.dir)
        lines.push({
            label: qsTr("Size"),
            value: Listing.size(row)
        });
    const times = [[qsTr("Created"), details.created_ms], [qsTr("Modified"), details.entry.modified_ms], [qsTr("Accessed"), details.accessed_ms]];
    for (const [label, ms] of times) {
        if (ms !== undefined)
            lines.push({
                label: label,
                value: Qt.formatDateTime(new Date(ms), "d MMM yyyy, HH:mm")
            });
    }
    return lines;
}

// Who may do what, each {who, role, can}: its owner, its group and
// everyone else, from its permission bits.
function access(details) {
    return [
        {
            who: details.owner,
            role: qsTr("Owner"),
            can: can(details.mode >> 6)
        },
        {
            who: details.group,
            role: qsTr("Group"),
            can: can(details.mode >> 3)
        },
        {
            who: qsTr("Others"),
            role: "",
            can: can(details.mode)
        }
    ];
}

// Three permission bits (read, write, execute) in words.
function can(bits) {
    const read = (bits & 4) !== 0;
    const write = (bits & 2) !== 0;
    const run = (bits & 1) !== 0;
    if (read && write && run)
        return qsTr("Read, write, and execute");
    if (read && write)
        return qsTr("Read and write");
    if (read && run)
        return qsTr("Read and execute");
    if (read)
        return qsTr("Read-only");
    if (write)
        return qsTr("Write-only");
    return qsTr("None");
}
