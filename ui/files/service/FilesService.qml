pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// Nova Files' link to novad: the windows it asks for, and opening files in
// apps (the default one on a double-click or Enter, or one from "Open
// with", which can also become the new default). The Nova Files
// process (../../files.qml) and its windows get these here and never talk
// to Daemon themselves.
Singleton {
    id: root

    // A window to open on `window.folder`, with `window.select` (absolute
    // paths in it, maybe none) selected.
    signal windowRequested(var window)

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
    }
}
