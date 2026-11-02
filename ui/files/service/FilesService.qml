pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// Nova Files' link to novad: the windows it asks for, opening files in
// apps (the default one on a double-click or Enter, or one from "Open
// with", which can also become the new default), and copy and move jobs. The Nova Files
// process (../../files.qml) and its windows get these here and never talk
// to Daemon themselves.
Singleton {
    id: root

    // A window to open on `window.folder`, with `window.select` (absolute
    // paths in it, maybe none) selected.
    signal windowRequested(var window)
    // How a copy or move job is going (see Daemon.filesTransferChanged).
    signal transferChanged(var transfer)

    // The number the next job gets; novad numbers jobs per connection.
    property int _nextJob: 1

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

    // Copies or moves (`mode`, a Protocol.TRANSFER_* mode) `sources`
    // (absolute paths) into the folder `folder`. Returns the job's number,
    // which its transferChanged events carry; `failed(error)` gets novad's
    // refusal, when it doesn't start.
    function transfer(mode: string, sources: var, folder: string, failed: var): int {
        const job = _nextJob++;
        Daemon.call(Protocol.CMD_FILES_TRANSFER, {
            job: job,
            mode: mode,
            sources: sources,
            folder: folder
        }, error => {
            if (error) {
                console.warn(`nova: copying into ${folder}: ${error}`);
                failed?.(error);
            }
        });
        return job;
    }

    // Stops job `job`; it then reports it was cancelled.
    function cancelTransfer(job: int) {
        Daemon.call(Protocol.CMD_FILES_TRANSFER_CANCEL, {
            job: job
        });
    }

    // Answers job `job`'s clash `clash` with `answer` (a Protocol.CLASH_*
    // answer), for the clashes like it too when `all`.
    function answerClash(job: int, clash: int, answer: string, all: bool) {
        Daemon.call(Protocol.CMD_FILES_TRANSFER_ANSWER, {
            job: job,
            clash: clash,
            answer: answer,
            all: all
        });
    }

    Connections {
        target: Daemon
        function onFilesWindowRequested(window: var) {
            root.windowRequested(window);
        }
        function onFilesTransferChanged(transfer: var) {
            root.transferChanged(transfer);
        }
        function onConnectedChanged() {
            if (Daemon.connected)
                root.attach();
        }
    }
}
