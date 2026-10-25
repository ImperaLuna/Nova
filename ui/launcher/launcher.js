.pragma library

// The launcher's logic that needs no Qt: what its list shows, moving the
// selection, and an app's stand-in when its icon is missing. The QML tests
// call these directly.

// How many swatches Tokens has (swatch1 .. swatch8).
var SWATCHES = 8;

// {id: app} for a launcher event's `apps`.
function byId(apps) {
    var map = {};
    for (var i = 0; i < apps.length; i++)
        map[apps[i].id] = apps[i];
    return map;
}

// The apps with these ids, in their order, skipping any not in `known`.
function pick(known, ids) {
    var apps = [];
    for (var i = 0; i < ids.length; i++) {
        if (known[ids[i]])
            apps.push(known[ids[i]]);
    }
    return apps;
}

// What the list shows: the matches while there's a query (the name in runs,
// matched letters marked), else the recent apps followed by every other app
// by name, so the arrows reach any app without typing. Each row is
// {id, name, runs, description, icon, group}, `group` being "results",
// "recent" or "apps": the list's headings.
function rows(query, matches, recent, apps, known) {
    function row(app, group, runs) {
        return {
            id: app.id,
            name: app.name,
            runs: runs ?? [
                {
                    text: app.name,
                    matched: false
                }
            ],
            description: app.description ?? "",
            icon: app.icon ?? "",
            group: group
        };
    }
    if (query.trim() !== "") {
        var found = [];
        for (var i = 0; i < matches.length; i++) {
            var app = known[matches[i].id];
            if (app)
                found.push(row(app, "results", matches[i].name.map(function (run) {
                    return {
                        text: run.text,
                        matched: run.matched === true
                    };
                })));
        }
        return found;
    }
    var shown = pick(known, recent).map(function (app) {
        return row(app, "recent");
    });
    var others = apps.filter(function (app) {
        return recent.indexOf(app.id) < 0;
    });
    others.sort(function (a, b) {
        return a.name.localeCompare(b.name);
    });
    return shown.concat(others.map(function (app) {
        return row(app, "apps");
    }));
}

// The selection after moving `by` rows from `index` among `count`, going
// round at either end.
function step(index, count, by) {
    if (count === 0)
        return 0;
    return ((index + by) % count + count) % count;
}

// The letter an app without an icon shows: its name's first letter or digit.
function letter(name) {
    for (var i = 0; i < name.length; i++) {
        var c = name[i];
        // Letters are what has an upper and a lower case.
        if (/[0-9]/.test(c) || c.toUpperCase() !== c.toLowerCase())
            return c.toUpperCase();
    }
    return "?";
}

// Which swatch (1 .. SWATCHES) an app's letter sits on: the same for the
// same id every time.
function swatch(id) {
    var hash = 0;
    for (var i = 0; i < id.length; i++)
        hash = (hash * 31 + id.charCodeAt(i)) | 0;
    return (Math.abs(hash) % SWATCHES) + 1;
}

// A file:// URL for an absolute path, each part escaped (spaces, #, ?).
function fileUrl(path) {
    return "file://" + path.split("/").map(encodeURIComponent).join("/");
}
