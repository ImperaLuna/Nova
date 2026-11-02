.pragma library
.import "../daemon/protocol.js" as Protocol
.import "../text/bytes.js" as Bytes

// Nova Files' copies and moves as its window shows them: the jobs it keeps
// from novad's files_transfer events, the progress card's words, and the
// name clash dialog's. No QML types, so ui/tests can check it all.

// How a clash dialog writes when each side was last changed.
var MODIFIED_FORMAT = "MMM d, yyyy, h:mm AP";

// What the clash dialog holds before its first clash.
var NO_CLASH = {
    id: 0,
    name: "",
    folder: false,
    others: 0,
    original: {},
    replacement: {}
};

// Whether a job in `state` has ended: its last event.
function ended(state) {
    return state === Protocol.TRANSFER_DONE || state === Protocol.TRANSFER_CANCELLED || state === Protocol.TRANSFER_FAILED;
}

// `jobs` (a list of transfers, oldest first) with `transfer` in it: a new
// job goes last, a known one is replaced where it is, and one that finished
// or was cancelled leaves. A failed one stays, showing why, until it's
// dismissed.
function apply(jobs, transfer) {
    var kept = jobs.filter(function (job) {
        return job.job !== transfer.job;
    });
    var gone = transfer.state === Protocol.TRANSFER_DONE || transfer.state === Protocol.TRANSFER_CANCELLED;
    if (gone)
        return kept;
    var at = jobs.findIndex(function (job) {
        return job.job === transfer.job;
    });
    var next = kept.slice();
    next.splice(at < 0 ? next.length : at, 0, transfer);
    return next;
}

// `jobs` without job `job`.
function without(jobs, job) {
    return jobs.filter(function (shown) {
        return shown.job !== job;
    });
}

// The first job waiting on a name clash, or null.
function asking(jobs) {
    return jobs.find(function (job) {
        return job.state === Protocol.TRANSFER_ASKING && job.clash;
    }) ?? null;
}

// What a job is copying: its one item's name, or how many.
function label(transfer) {
    return transfer.items === 1 ? transfer.name : qsTr("%1 items").arg(transfer.items);
}

// How far along a job is, 0 to 1.
function fraction(transfer) {
    return transfer.total_bytes > 0 ? Math.min(1, transfer.done_bytes / transfer.total_bytes) : 0;
}

function percent(transfer) {
    return Math.floor(fraction(transfer) * 100) + "%";
}

// The clash dialog's title.
function clashTitle(clash) {
    return qsTr("“%1” already exists in this location").arg(clash.name);
}

// The line under it: what the default answer does.
function clashQuestion(clash) {
    return clash.folder ? qsTr("Do you want to merge them? Files with the same name will be asked about.") : qsTr("Do you want to replace it? Replacing it will overwrite its content.");
}

// The names of the two sides, the one there now first.
function originalLabel(clash) {
    return clash.folder ? qsTr("Original folder") : qsTr("Original file");
}

function replacementLabel(clash) {
    return clash.folder ? qsTr("Merge with") : qsTr("Replace with");
}

// A side's size, or for a folder how much it holds.
function amount(item) {
    return item.items !== undefined ? qsTr("Contents: %1 items").arg(item.items) : qsTr("Size: %1").arg(Bytes.bytes(item.size ?? 0));
}

// When a side was last changed, or "" when novad couldn't tell.
function modified(item) {
    if (item.modified_ms === undefined)
        return "";
    return qsTr("Last modified: %1").arg(Qt.formatDateTime(new Date(item.modified_ms), MODIFIED_FORMAT));
}

// "Apply to all (2)": the clash and the others like it.
function allLabel(clash) {
    return qsTr("Apply to all (%1)").arg(clash.others);
}

// The answer the dialog's default button gives, and its word.
function defaultAnswer(clash) {
    return clash.folder ? Protocol.CLASH_MERGE : Protocol.CLASH_REPLACE;
}

function defaultLabel(clash) {
    return clash.folder ? qsTr("Merge") : qsTr("Replace");
}
