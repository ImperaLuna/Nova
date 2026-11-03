pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol
import "../trash.js" as Trash

// Nova Files' link to novad: the windows it asks for, the folders they
// show (folder views: each FolderModel opens one under its own number and
// gets its pages, changes and closing here), the sidebar's tree and
// places, the drives (and mounting and ejecting them), opening files in
// apps (the default one on a double-click or Enter, or one from "Open
// with", which can also become the new default), copy and move jobs, and
// the trash. The Nova Files process (../../files.qml) and its windows get
// these here and never talk to Daemon themselves. Also how solid the
// windows are, from config.toml's [files].
Singleton {
    id: root

    // How solid the window frame and the file list are, each 0 (glass,
    // also until novad says) to 1 (the theme's solid surfaces).
    readonly property real frameSolid: (Daemon.filesLook?.frame ?? 0) / Protocol.FILES_SOLID_MAX
    readonly property real listSolid: (Daemon.filesLook?.list ?? 0) / Protocol.FILES_SOLID_MAX
    // The drives for the sidebar, in its order: {id, name, removable,
    // mount (absent while it isn't mounted), size, used (bytes, while
    // mounted)}.
    readonly property var drives: Daemon.filesDrives

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
    // How a copy or move job is going (see Daemon.filesTransferChanged).
    signal transferChanged(var transfer)
    // The connection to novad dropped, and novad stopped every job with it.
    signal transfersLost

    // Something in the trash changed: ask trashList again.
    signal trashChanged

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

    // `done(error, tree)` gets the sidebar's tree for a window on `folder`
    // (absolute, or from `~`): {folder, rows}, with `folder` absolute. The
    // tree is open down to `folder` but at the folders in `closed`, and at
    // those in `opened`; hidden folders show off the way only when `hidden`.
    // An error means `folder` isn't a folder.
    function tree(folder: string, opened: var, closed: var, hidden: bool, done: var) {
        Daemon.call(Protocol.CMD_FILES_TREE, {
            folder: folder,
            opened: opened,
            closed: closed,
            hidden: hidden
        }, (error, result) => done(error ?? null, result ?? null));
    }

    // `done(places)` gets the sidebar's places that exist, in order, each
    // {kind, path}; none when novad couldn't tell.
    function places(done: var) {
        Daemon.call(Protocol.CMD_FILES_PLACES, undefined, (error, result) => {
            if (error)
                console.warn(`nova: sidebar places: ${error}`);
            done(error ? [] : result);
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

    // Copies or moves (`mode`, a Protocol.TRANSFER_* mode) `sources`
    // (absolute paths) into the folder `folder`. Returns the job's number,
    // which its transferChanged events carry; `failed(job, error)` gets
    // novad's refusal when it doesn't start, maybe before this returns.
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
                failed?.(job, error);
            }
        });
        return job;
    }

    // Whether every item at `sources` (absolute paths) is on the same
    // drive as the folder `folder`: a drop there moves them, elsewhere it
    // copies. `done(same)` gets the answer, or null when novad can't tell.
    function sameDrive(sources: var, folder: string, done: var) {
        Daemon.call(Protocol.CMD_FILES_SAME_DRIVE, {
            sources: sources,
            folder: folder
        }, (error, same) => {
            if (error)
                console.warn(`nova: checking the drive of ${folder}: ${error}`);
            done(error ? null : same);
        });
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

    // Moves `paths` (absolute) to the trash. `done(error)` gets novad's
    // answer: null, or which ones didn't go.
    function trash(paths: var, done: var) {
        _inBatches(Protocol.CMD_FILES_TRASH, "paths", paths, "moving to the trash", done);
    }

    // Deletes `paths` (absolute) for good.
    function deleteForGood(paths: var, done: var) {
        _inBatches(Protocol.CMD_FILES_DELETE, "paths", paths, "deleting", done);
    }

    // `done(items)` gets what the trash holds, the last trashed first, each
    // {id, folder, deleted_ms, entry}; none when novad couldn't tell.
    function trashList(done: var) {
        Daemon.call(Protocol.CMD_FILES_TRASH_LIST, undefined, (error, result) => {
            if (error)
                console.warn(`nova: the trash: ${error}`);
            done(error ? null : result);
        });
    }

    // Puts the trashed items `ids` back where they came from.
    function restore(ids: var, done: var) {
        Daemon.call(Protocol.CMD_FILES_RESTORE, {
            ids: ids
        }, error => root._answered("restoring", error, done));
    }

    // Deletes the trashed items `ids` for good.
    function eraseFromTrash(ids: var, done: var) {
        _inBatches(Protocol.CMD_FILES_TRASH_DELETE, "ids", ids, "deleting from the trash", done);
    }

    // Deletes everything in the trash for good.
    function emptyTrash(done: var) {
        Daemon.call(Protocol.CMD_FILES_EMPTY_TRASH, undefined, error => root._answered("emptying the trash", error, done));
    }

    // Sends `cmd` with `items` under `key`, in as many requests as keep each
    // under novad's limit (Trash.batches), one after another. `done(error)`
    // gets every request's errors at once, or null.
    function _inBatches(cmd: string, key: string, items: var, doing: string, done: var) {
        const batches = Trash.batches(items, Protocol.REQUEST_MAX_BYTES);
        const errors = [];
        const next = index => {
            if (index === batches.length) {
                root._answered(doing, errors.length > 0 ? errors.join("\n") : null, done);
                return;
            }
            const args = {};
            args[key] = batches[index];
            Daemon.call(cmd, args, error => {
                if (error)
                    errors.push(error);
                next(index + 1);
            });
        };
        next(0);
    }

    function _answered(doing: string, error: var, done: var) {
        if (error)
            console.warn(`nova: ${doing}: ${error}`);
        done?.(error ?? null);
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
        function onFilesTransferChanged(transfer: var) {
            root.transferChanged(transfer);
        }
        function onFilesTrashChanged() {
            root.trashChanged();
        }
        function onConnectedChanged() {
            if (Daemon.connected)
                root.attach();
            else
                root.transfersLost();
        }
    }
}
