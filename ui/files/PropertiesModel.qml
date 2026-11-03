import QtQuick
import "service"
import "properties.js" as Props

// What a window's Properties dialog shows, asked of novad: the one item's
// facts (files_properties), its size counted in the background
// (files_measure, summed over its jobs as they report), and the apps that
// open a file. `show(subject)` starts on a Props.subject; `close()` stops
// counting. Only answers to the latest subject count, so a late one for an
// item shown before is dropped.
QtObject {
    id: model

    // Props.subject(...), or null while nothing is shown.
    property var subject: null
    property var properties: null
    property var counted: null
    readonly property OpenWithModel openWith: OpenWithModel {
        path: model.subject && !model.subject.drive && model.subject.rows.length === 1 && !model.subject.rows[0].dir ? model.subject.paths[0] : ""
    }

    // The measuring jobs, and each one's latest counts by job.
    property var _jobs: []
    property var _counts: ({})
    property int _asked: 0

    function show(shown: var) {
        close();
        subject = shown;
        _ask();
        if (Props.measures(shown)) {
            _counts = {};
            _jobs = FilesService.measure(shown.paths);
            counted = Props.total(_counts, _jobs);
        }
    }

    function close() {
        for (const job of _jobs)
            FilesService.cancelMeasure(job);
        _jobs = [];
        _counts = {};
        subject = null;
        properties = null;
        counted = null;
        _asked++;
    }

    // Gives or takes `who`'s `right` (Props.flipped), then reads the item
    // again to show what took.
    function flipAccess(who: string, right: string) {
        if (!properties)
            return;
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
            if (asked === model._asked)
                model.properties = properties;
        });
    }

    readonly property Connections _measured: Connections {
        target: FilesService
        function onMeasured(measured: var) {
            if (!model._jobs.includes(measured.job))
                return;
            const counts = Object.assign({}, model._counts);
            counts[measured.job] = measured;
            model._counts = counts;
            model.counted = Props.total(counts, model._jobs);
        }
    }
}
