import QtQuick
import "service"
import "properties.js" as Props
import "trash.js" as Trash
import "../daemon/protocol.js" as Protocol

// What a window's Properties dialog shows, asked of novad: the one item's
// facts (files_properties), its size counted in the background
// (files_measure: a big selection in batches that each fit a request, one
// job at a time, summed as they report), and the apps that open a file.
// `show(subject)` starts on a Props.subject; `close()`, or the window
// going, stops counting. Only answers to the latest subject count, so a
// late one for an item shown before is dropped. While a change to who may
// open it is on its way (`changing`), no other is sent, so each starts
// from what the last one left.
QtObject {
    id: model

    // Props.subject(...), or null while nothing is shown.
    property var subject: null
    property var properties: null
    property var counted: null
    property bool changing: false
    readonly property OpenWithModel openWith: OpenWithModel {
        path: model.subject && !model.subject.drive && model.subject.rows.length === 1 && !model.subject.rows[0].dir ? model.subject.paths[0] : ""
    }

    // The measuring jobs started, each one's latest counts by job, and the
    // batches of paths still to measure.
    property var _jobs: []
    property var _counts: ({})
    property var _batches: []
    property int _asked: 0

    function show(shown: var) {
        close();
        subject = shown;
        _ask();
        if (Props.measures(shown)) {
            _batches = Trash.batches(shown.paths, Protocol.REQUEST_MAX_BYTES);
            _next();
        }
    }

    function close() {
        for (const job of _jobs)
            FilesService.cancelMeasure(job);
        _jobs = [];
        _counts = {};
        _batches = [];
        subject = null;
        properties = null;
        counted = null;
        changing = false;
        _asked++;
    }

    // Gives or takes `who`'s `right` (Props.flipped), then reads the item
    // again to show what took.
    function flipAccess(who: string, right: string) {
        if (!properties || changing)
            return;
        changing = true;
        const path = subject.paths[0];
        const asked = _asked;
        FilesService.setAccess(path, Props.flipped(properties.access, who, right), () => {
            if (asked === model._asked)
                model._ask();
        });
    }

    // Reads the one item (a selection has no facts of its own).
    function _ask() {
        if (!subject || subject.rows.length > 1)
            return;
        const asked = ++_asked;
        FilesService.properties(subject.paths[0], properties => {
            if (asked !== model._asked)
                return;
            model.properties = properties;
            model.changing = false;
        });
    }

    // Starts the next batch's job.
    function _next() {
        const batch = _batches[0];
        _batches = _batches.slice(1);
        _jobs = _jobs.concat([FilesService.measure(batch)]);
        _sum();
    }

    function _sum() {
        const total = Props.total(_counts, _jobs);
        total.done = total.done && _batches.length === 0;
        counted = total;
    }

    readonly property Connections _measured: Connections {
        target: FilesService
        function onMeasured(measured: var) {
            if (!model._jobs.includes(measured.job))
                return;
            const counts = Object.assign({}, model._counts);
            counts[measured.job] = measured;
            model._counts = counts;
            if (measured.done && model._batches.length > 0)
                model._next();
            else
                model._sum();
        }
    }

    Component.onDestruction: close()
}
