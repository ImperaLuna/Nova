.pragma library
.import "../text/bytes.js" as Bytes
.import "../theme/icons.js" as Icons
.import "listing.js" as Listing
.import "paths.js" as Paths
.import "transfers.js" as Transfers

// Nova Files' Properties window as it reads, kept apart so ui/tests can
// check it: after Explorer's General page, one item's facts (novad's
// files_properties answer) or a selection's totals, the size counted so
// far (files_measure's counts, summed over its jobs), and who may read,
// change and run it.

// What a window shows Properties for: `rows` (the file list's, plain
// copies) in `folder`, or the folder itself when there are none, or a
// drive ({name, mount}).
function subject(folder, rows, drive) {
    if (drive)
        return {
            paths: [drive.mount],
            rows: [],
            title: drive.name,
            drive: true
        };
    if (rows.length === 0)
        return {
            paths: [folder],
            rows: [],
            title: Paths.folderName(folder),
            drive: false
        };
    return {
        paths: rows.map(row => Paths.child(folder, row.name)),
        rows: rows,
        title: rows.length === 1 ? Paths.shown(rows[0].name) : items(rows.length),
        drive: false
    };
}

// Whether the size is counted: never for a drive (its fill says it), nor
// for one file, whose own size is all there is.
function measures(subject) {
    if (subject.drive)
        return false;
    return subject.rows.length !== 1 || subject.rows[0].dir;
}

// The big icon: a drive, many files, or the one item's.
function icon(subject, properties) {
    if (subject.drive)
        return Icons.DISK;
    if (subject.rows.length > 1)
        return Icons.COPY;
    const entry = properties?.entry;
    return entry ? Listing.icon(Listing.row(entry)) : Icons.FOLDER;
}

// "1 item", "12 items".
function items(count) {
    return count === 1 ? qsTr("1 item") : qsTr("%1 items").arg(count);
}

// A size as Explorer shows it: "1.4 GB (1,400,000,000 bytes)", or just
// "12 bytes" when that's all.
function size(bytes) {
    const exact = bytes === 1 ? qsTr("1 byte") : qsTr("%1 bytes").arg(grouped(bytes));
    return bytes < BYTES_SHORT_FROM ? exact : qsTr("%1 (%2)").arg(Bytes.bytes(bytes)).arg(exact);
}

// Below this a size reads in bytes only.
var BYTES_SHORT_FROM = 1000;

// `count` with thousands grouped: "1,400,000".
function grouped(count) {
    return String(Math.round(count)).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

// "120 files, 14 folders" (each left out when there are none; "Empty"
// when both are).
function contains(files, folders) {
    const parts = [];
    if (files > 0)
        parts.push(files === 1 ? qsTr("1 file") : qsTr("%1 files").arg(grouped(files)));
    if (folders > 0)
        parts.push(folders === 1 ? qsTr("1 folder") : qsTr("%1 folders").arg(grouped(folders)));
    return parts.length > 0 ? parts.join(", ") : qsTr("Empty");
}

// A time in milliseconds since 1970, or "" when novad didn't say.
function time(ms) {
    return ms === undefined || ms === null || ms < 0 ? "" : Qt.formatDateTime(new Date(ms), Transfers.MODIFIED_FORMAT);
}

// The sum of a measure's jobs' latest counts ({job: counted}): {files,
// folders, bytes, disk_bytes, done}; done once every one of `jobs` is.
function total(counts, jobs) {
    const sum = {
        files: 0,
        folders: 0,
        bytes: 0,
        disk_bytes: 0,
        done: jobs.length > 0
    };
    for (const job of jobs) {
        const counted = counts[job];
        if (!counted) {
            sum.done = false;
            continue;
        }
        sum.files += counted.files;
        sum.folders += counted.folders;
        sum.bytes += counted.bytes;
        sum.disk_bytes += counted.disk_bytes;
        sum.done = sum.done && counted.done;
    }
    return sum;
}

// The label and value rows, in groups (a rule between each): [[{label,
// value, change, counting}]], `change` on "Opens with", which offers
// other apps, `counting` on sizes still being counted. `properties` is novad's answer for one item (null for a
// selection or while it's on its way), `counted` the measure's total
// (null when not measured), `openWith` the app that opens it ({name}, or
// null).
function groups(subject, properties, counted, openWith) {
    const pair = (label, value) => ({
                label: label,
                value: value
            });
    const kept = rows => rows.filter(row => row.value !== "");
    if (subject.drive) {
        const drive = properties?.drive;
        if (!drive)
            return [kept([pair(qsTr("Location"), Paths.shown(subject.paths[0]))])];
        return [kept([pair(qsTr("Type"), qsTr("Drive")), pair(qsTr("Location"), Paths.shown(subject.paths[0]))]), [pair(qsTr("Used"), size(drive.used)), pair(qsTr("Free"), size(Math.max(drive.size - drive.used, 0))), pair(qsTr("Capacity"), size(drive.size))]];
    }
    if (subject.rows.length > 1)
        return selectionGroups(subject, counted);
    const entry = properties?.entry;
    const dir = entry?.dir ?? true;
    const sizes = [];
    if (counted) {
        sizes.push(pair(qsTr("Size"), size(counted.bytes)), pair(qsTr("Size on disk"), size(counted.disk_bytes)));
        if (dir)
            sizes.push(pair(qsTr("Contains"), contains(counted.files, counted.folders)));
        for (const row of sizes)
            row.counting = !counted.done;
    } else if (entry && entry.size !== undefined) {
        sizes.push(pair(qsTr("Size"), size(entry.size)));
        if (properties.disk_bytes !== undefined)
            sizes.push(pair(qsTr("Size on disk"), size(properties.disk_bytes)));
    }
    const facts = [pair(qsTr("Type"), entry ? Listing.kind(Listing.row(entry)) : "")];
    if (openWith)
        facts.push({
            label: qsTr("Opens with"),
            value: openWith.name,
            change: true
        });
    if (properties?.link_target !== undefined && properties?.link_target !== null)
        facts.push(pair(qsTr("Points to"), Paths.shown(properties.link_target)));
    facts.push(pair(qsTr("Location"), properties ? Paths.shown(properties.folder) : ""));
    const times = [pair(qsTr("Created"), time(properties?.created_ms)), pair(qsTr("Modified"), time(entry?.modified_ms)), pair(qsTr("Accessed"), time(properties?.accessed_ms))];
    return [kept(facts), kept(sizes), kept(times)].filter(group => group.length > 0);
}

// A selection's groups: what kind, where, and what it adds up to.
function selectionGroups(subject, counted) {
    const rows = subject.rows;
    const folders = rows.filter(row => row.dir).length;
    const kinds = new Set(rows.map(row => Listing.kind(row)));
    const kind = kinds.size === 1 ? qsTr("All %1").arg(Array.from(kinds)[0]) : qsTr("Multiple types");
    const folder = Paths.parent(subject.paths[0]);
    const facts = [{
            label: qsTr("Type"),
            value: kind
        }, {
            label: qsTr("Location"),
            value: qsTr("All in %1").arg(Paths.shown(folder))
        }];
    const sizes = [];
    if (counted)
        sizes.push({
            label: qsTr("Size"),
            value: size(counted.bytes)
        }, {
            label: qsTr("Size on disk"),
            value: size(counted.disk_bytes)
        }, {
            label: qsTr("Contains"),
            value: contains(rows.length - folders + counted.files, folders + counted.folders)
        });
    for (const row of sizes)
        row.counting = !counted.done;
    return sizes.length > 0 ? [facts, sizes] : [facts];
}

// The access grid's rows: the owner, the group and everyone else, each
// {who, name, key, rights}; `key` names it in an access object.
function people(properties) {
    if (!properties)
        return [];
    const access = properties.access;
    return [
        {
            who: qsTr("Owner"),
            name: properties.owner,
            key: "owner",
            rights: access.owner
        },
        {
            who: qsTr("Group"),
            name: properties.group,
            key: "group",
            rights: access.group
        },
        {
            who: qsTr("Everyone"),
            name: "",
            key: "others",
            rights: access.others
        }
    ];
}

// The access grid's columns: [{key, label}]; a folder's run is going in.
function rightsColumns(dir) {
    return [
        {
            key: "read",
            label: qsTr("Read")
        },
        {
            key: "write",
            label: qsTr("Change")
        },
        {
            key: "run",
            label: dir ? qsTr("Open") : qsTr("Run")
        }
    ];
}

// `access` with `who`'s `right` flipped, as files_set_access takes it.
function flipped(access, who, right) {
    const next = {
        owner: Object.assign({}, access.owner),
        group: Object.assign({}, access.group),
        others: Object.assign({}, access.others)
    };
    next[who][right] = !next[who][right];
    return next;
}

// How full a drive is, 0 to 1.
function fill(drive) {
    return drive && drive.size > 0 ? Math.min(drive.used / drive.size, 1) : 0;
}
