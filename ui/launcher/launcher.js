.pragma library
.import "../daemon/protocol.js" as Protocol

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

// What the list shows: what the providers found while there's a query (the
// name in runs, matched letters marked), else the recent apps followed by
// every other app by name, so the arrows reach any app without typing; in
// the Files mode (`files`), only what was found. Each row is {provider, id,
// name, runs, description, icon, file, group}, `file` being a found file's
// LauncherFile (else null) and `group` "results", "files", "recent" or
// "apps": the list's headings.
function rows(query, matches, recent, apps, known, files) {
    function row(app, group, runs, provider, file) {
        return {
            provider: provider ?? Protocol.PROVIDER_APPS,
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
            file: file ?? null,
            group: group
        };
    }
    if (files && query.trim() === "")
        return [];
    if (query.trim() !== "") {
        return matches.map(function (item) {
            var name = item.name.map(function (run) {
                return run.text;
            }).join("");
            return row({
                id: item.id,
                name: name,
                description: item.description,
                icon: item.icon
            }, files ? "files" : "results", item.name.map(function (run) {
                return {
                    text: run.text,
                    matched: run.matched === true
                };
            }), item.provider, item.file);
        });
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

// The query starts a path (`~` or `/`), which the Files mode completes.
function typesPath(query) {
    return query.startsWith("~") || query.startsWith("/");
}

// What Tab would add to `query` to complete it to `row`'s path: the rest of
// its completion when the query starts it (case aside), else "".
function completion(query, row) {
    const full = row?.file?.completion ?? "";
    if (full.length <= query.length || full.slice(0, query.length).toLowerCase() !== query.toLowerCase())
        return "";
    return full.slice(query.length);
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

