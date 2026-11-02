pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// Nova Files' link to novad: the windows it asks for, the folders they
// show (folder views: each FolderModel opens one under its own number and
// gets its pages, changes and closing here), and opening files in apps
// (the default one on a double-click or Enter, or one from "Open with",
// which can also become the new default). The Nova Files
// process (../../files.qml) and its windows get these here and never talk
// to Daemon themselves. Also how solid the windows are, from
// config.toml's [files].
Singleton {
    id: root

    // How solid the window frame and the file list are, each 0 (glass,
    // also until novad says) to 1 (the theme's solid surfaces).
    readonly property real frameSolid: (Daemon.filesLook?.frame ?? 0) / Protocol.FILES_SOLID_MAX
    readonly property real listSolid: (Daemon.filesLook?.list ?? 0) / Protocol.FILES_SOLID_MAX

    // A window to open on `window.folder`, with `window.select` (absolute
    // paths in it, maybe none) selected.
    signal windowRequested(var window)
    // A folder view's page of entries, its changes and novad closing it,
    // for every view: each FolderModel picks its own by `view`.
    signal page(var page)
    signal changed(var changed)
    signal viewClosed(var closed)

    // novad drops a connection's views when it goes: views open again on
    // the next connection.
    readonly property bool connected: Daemon.connected

    property int _nextView: 1

    // A number for a new folder view, never handed out twice.
    function newView(): int {
        return _nextView++;
    }

    // Shows the folder at `path` (absolute) in view `view`, ordered by
    // `sortKey` (a Protocol.FILE_SORT_*), with dot files when `hidden`.
    // Whatever the view showed is replaced; its entries come as `page`s.
    // `done(error)` gets novad's answer.
    function open(view: int, path: string, sortKey: string, descending: bool, hidden: bool, done: var) {
        Daemon.call(Protocol.CMD_FILES_OPEN, {
            view: view,
            path: path,
            sort: {
                key: sortKey,
                descending: descending
            },
            hidden: hidden
        }, error => {
            if (error)
                console.warn(`nova: opening the folder ${path}: ${error}`);
            done?.(error ?? null);
        });
    }

    // Closes view `view`; nothing more comes for it.
    function close(view: int) {
        Daemon.call(Protocol.CMD_FILES_CLOSE, {
            view: view
        });
    }

    // Tells novad this process opens Nova Files' windows; done again on
    // every connection.
    function attach() {
        Daemon.call(Protocol.CMD_ATTACH_FILES, {
            attached: true
        });
    }

    // Tells novad to send no more windows, then calls `done`, even when
    // novad isn't there.
    function detach(done) {
        Daemon.call(Protocol.CMD_ATTACH_FILES, {
            attached: false
        }, () => done());
    }

    // Opens the file at `path` (absolute) in the app `app` (a desktop file
    // id) or, when `app` is empty, in its type's default app: a double-click
    // or Enter on a file. `done(error)`, when given, gets novad's answer.
    function launch(path: string, app: string, done: var) {
        const args = app ? {
            path: path,
            app: app
        } : {
            path: path
        };
        Daemon.call(Protocol.CMD_FILES_LAUNCH, args, error => {
            if (error)
                console.warn(`nova: opening ${path}: ${error}`);
            done?.(error ?? null);
        });
    }

    // `done(openWith)` gets what can open the file at `path`: {mime,
    // default, apps}, the default first among apps ({id, name, icon}), or
    // null when novad couldn't tell.
    function openWith(path: string, done: var) {
        Daemon.call(Protocol.CMD_FILES_OPEN_WITH, {
            path: path
        }, (error, result) => {
            if (error)
                console.warn(`nova: apps for ${path}: ${error}`);
            done(error ? null : result);
        });
    }

    // Makes the app `app` the default for files of the type of the one at
    // `path`. `done(error)` gets novad's answer.
    function setDefault(path: string, app: string, done: var) {
        Daemon.call(Protocol.CMD_FILES_SET_DEFAULT, {
            path: path,
            app: app
        }, error => {
            if (error)
                console.warn(`nova: default for ${path}: ${error}`);
            done?.(error ?? null);
        });
    }

    Connections {
        target: Daemon
        function onFilesWindowRequested(window: var) {
            root.windowRequested(window);
        }
        function onFilesPage(page: var) {
            root.page(page);
        }
        function onFilesChanged(changed: var) {
            root.changed(changed);
        }
        function onFilesClosed(closed: var) {
            root.viewClosed(closed);
        }
        function onConnectedChanged() {
            if (Daemon.connected)
                root.attach();
        }
    }
}
