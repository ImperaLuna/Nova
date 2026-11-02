import QtQuick
import "service"

// "Open with" for one file, for the right-click menu and the details pane
// to show: the apps that can open `path`, its type's default first, as
// novad worked them out. `open(app)` opens the file in one; `makeDefault(app)`
// makes one the default for files of this type and reads the apps again.
// An answer for a path the model no longer shows is dropped.
QtObject {
    id: model

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

    function reload() {
        const asked = path;
        mime = "";
        defaultApp = "";
        apps = [];
        loading = asked !== "";
        if (!loading)
            return;
        FilesService.openWith(asked, openWith => {
            if (asked !== model.path)
                return;
            model.mime = openWith?.mime ?? "";
            model.defaultApp = openWith?.default ?? "";
            model.apps = openWith?.apps ?? [];
            model.loading = false;
        });
    }

    function open(app: string) {
        FilesService.launch(path, app, null);
    }

    function makeDefault(app: string) {
        FilesService.setDefault(path, app, error => {
            if (!error)
                model.reload();
        });
    }

    onPathChanged: reload()
    Component.onCompleted: reload()
}
