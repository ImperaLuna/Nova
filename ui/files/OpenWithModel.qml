import QtQuick

// "Open with" for one file, for the right-click menu and the details pane
// to show: the apps that can open `path`, its type's default first, as
// novad worked them out. `open(app)` opens the file in one; `makeDefault(app)`
// makes one the default for files of this type and reads the apps again.
// Only the answer to the latest request counts. It asks through `service`
// (FilesService, given by the window), so tests can make one without
// novad.
QtObject {
    id: model

    // What it asks novad through: openWith(path, done), launch(path, app,
    // done) and setDefault(path, app, done).
    property var service: null
    // The file, an absolute path; empty for none.
    property string path: ""
    // Its type, like "image/png", once known.
    property string mime: ""
    // The desktop file id of the app that opens it by default, or "".
    property string defaultApp: ""
    // The apps that can open it ({id, name, icon}), the default first.
    property var apps: []
    // Waiting for novad's answer.
    property bool loading: false
    // Counts requests, so an older answer arriving late is dropped.
    property int _asked: 0

    function reload() {
        const asked = ++_asked;
        mime = "";
        defaultApp = "";
        apps = [];
        loading = path !== "" && service !== null;
        if (!loading)
            return;
        service.openWith(path, openWith => {
            if (asked !== model._asked)
                return;
            model.mime = openWith?.mime ?? "";
            model.defaultApp = openWith?.default ?? "";
            model.apps = openWith?.apps ?? [];
            model.loading = false;
        });
    }

    function open(app: string) {
        service?.launch(path, app, null);
    }

    function makeDefault(app: string) {
        service?.setDefault(path, app, error => {
            if (!error)
                model.reload();
        });
    }

    onPathChanged: reload()
    onServiceChanged: reload()
    Component.onCompleted: reload()
}
