pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// Nova Files' link to novad: the windows it asks for, the drives (and
// mounting and ejecting them), and opening files in apps (the default one
// on a double-click or Enter, or one from "Open with", which can also
// become the new default). The Nova Files process (../../files.qml) and
// its windows get these here and never talk to Daemon themselves.
Singleton {
    id: root

    // The drives for the sidebar, in its order: {id, name, removable,
    // mount (absent while it isn't mounted), size, used (bytes, while
    // mounted)}.
    readonly property var drives: Daemon.filesDrives

    // A window to open on `window.folder`, with `window.select` (absolute
    // paths in it, maybe none) selected.
    signal windowRequested(var window)

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

    // Mounts the drive `id` unless it is; `done(folder)` gets where it's
    // mounted, or null when it couldn't be.
    function mount(id: string, done: var) {
        Daemon.call(Protocol.CMD_FILES_MOUNT, {
            drive: id
        }, (error, folder) => {
            if (error)
                console.warn(`nova: mounting ${id}: ${error}`);
            done(error ? null : folder);
        });
    }

    // Ejects the removable drive `id`. `done(outcome)` gets
    // Protocol.EJECT_EJECTED, Protocol.EJECT_BUSY (something still uses it),
    // or null when it failed otherwise.
    function eject(id: string, done: var) {
        Daemon.call(Protocol.CMD_FILES_EJECT, {
            drive: id
        }, (error, outcome) => {
            if (error)
                console.warn(`nova: ejecting ${id}: ${error}`);
            done(error ? null : outcome);
        });
    }

    Connections {
        target: Daemon
        function onFilesWindowRequested(window: var) {
            root.windowRequested(window);
        }
        function onConnectedChanged() {
            if (Daemon.connected)
                root.attach();
        }
    }
}
