.pragma library
.import "../text/bytes.js" as Bytes
.import "paths.js" as Paths
.import "../daemon/protocol.js" as Protocol

// What uses space in Nova Files, kept apart so ui/tests can check it: how
// full a drive is and how that reads ("197 GB free of 931 GB", Explorer's
// wording and its 1024-based units, like every size in Nova), which drive a folder is on, the order "What uses space" lists
// a folder in, and how long each row's bar is. Drives are novad's
// ({id, name, removable, mount, size, used}).

// Below this share free, a drive's bar turns to the warning colour, as
// Explorer's does.
var LOW_FREE = 0.1;

// Biggest first, folders and files together.
var BIGGEST_FIRST = {
    key: Protocol.FILE_SORT_SIZE,
    descending: true,
    mixed: true
};

// novad knows how full the drive is: it's mounted and has a size.
function measured(drive) {
    return drive.mount !== undefined && drive.used !== undefined && drive.size > 0;
}

// How full the drive is, 0..1; 0 while it isn't measured.
function fill(drive) {
    return measured(drive) ? Math.min(1, drive.used / drive.size) : 0;
}

// Less than LOW_FREE of the drive is free.
function low(drive) {
    return measured(drive) && (drive.size - drive.used) / drive.size < LOW_FREE;
}

// "197 GB free of 931 GB"; empty while it isn't measured.
function freeOf(drive) {
    if (!measured(drive))
        return "";
    const free = Math.max(0, drive.size - drive.used);
    return qsTr("%1 free of %2").arg(Bytes.bytes(free)).arg(Bytes.bytes(drive.size));
}

// The mounted drive `folder` is on: the one with the longest mount that
// holds it, since one may be mounted inside another (/ and /mnt/games).
// Null when none does.
function driveOf(drives, folder) {
    let best = null;
    for (const drive of drives)
        if (drive.mount !== undefined && Paths.inside(folder, drive.mount) && (best === null || drive.mount.length > best.mount.length))
            best = drive;
    return best;
}

// How long a row's bar is, 0..1, against the biggest row's size; 0 for a
// row whose size isn't known (-1, a folder not counted yet).
function share(size, biggest) {
    return size > 0 && biggest > 0 ? Math.min(1, size / biggest) : 0;
}
