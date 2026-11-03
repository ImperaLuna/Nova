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
// with", which can also become the new default), copy and move jobs, the
// trash, new folders, renaming, an item's details, and the Properties
// window's facts, sizes and access. The Nova Files process
// (../../files.qml) and its windows get these here and never talk to
// Daemon themselves. Also how solid the windows are, from config.toml's
// [files].
Singleton {
    id: root

    // How solid the window frame and the file list are, each 0 (glass,
    // also until novad says) to 1 (the theme's solid surfaces).
    readonly property real frameSolid: (Daemon.filesLook?.frame ?? 0) / Protocol.FILES_SOLID_MAX
    readonly property real listSolid: (Daemon.filesLook?.list ?? 0) / Protocol.FILES_SOLID_MAX
    // What Ctrl+Z and Ctrl+Y would undo and redo now, in any window: {undo,
    // redo}, each a Protocol.FILE_STEP_* or absent when there's nothing.
    readonly property var history: Daemon.filesHistory ?? ({})
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

    // A number for a new copy or move job, never handed out twice.
    function newJob(): int {
        return _nextJob++;
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
        const job = newJob();
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

    // What dropping `sources` (absolute paths) on the folder `folder`
    // does, Ctrl held when `copy`: `done(mode)` gets a Protocol.TRANSFER_*
    // mode, or null when nothing can be dropped there (or novad can't tell).
    function drop(sources: var, folder: string, copy: bool, done: var) {
        Daemon.call(Protocol.CMD_FILES_DROP, {
            sources: sources,
            folder: folder,
            copy: copy
        }, (error, mode) => {
            if (error)
                console.warn(`nova: dropping into ${folder}: ${error}`);
            done(error ? null : mode ?? null);
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

    // Makes a new folder in `folder` (absolute). `done(error, name)` gets
    // novad's answer: the folder's name ("New folder", or numbered).
    function newFolder(folder: string, done: var) {
        Daemon.call(Protocol.CMD_FILES_NEW_FOLDER, {
            folder: folder
        }, (error, name) => root._answered("making a folder", error, error => done?.(error, name ?? "")));
    }

    // Renames the item at `path` (absolute) to `name`. `done(error, name)`
    // gets novad's answer: the name it now has.
    function rename(path: string, name: string, done: var) {
        Daemon.call(Protocol.CMD_FILES_RENAME, {
            path: path,
            name: name
        }, (error, renamed) => root._answered("renaming", error, error => done?.(error, renamed ?? "")));
    }

    // `done(details)` gets what the details pane shows about the item at
    // `path` (absolute): {entry, items, created_ms, accessed_ms, owner,
    // group, mode}, or null when novad couldn't tell.
    function details(path: string, done: var) {
        Daemon.call(Protocol.CMD_FILES_DETAILS, {
            path: path
        }, (error, result) => {
            if (error)
                console.warn(`nova: details of ${path}: ${error}`);
            done(error ? null : result);
        });
    }

    // How far a measuring job got (see Daemon.filesMeasured).
    signal measured(var measured)

    // `done(properties)` gets what the Properties window shows about the
    // item at `path` (absolute): novad's files_properties answer, or null
    // when it couldn't be read.
    function properties(path: string, done: var) {
        Daemon.call(Protocol.CMD_FILES_PROPERTIES, {
            path: path
        }, (error, result) => {
            if (error)
                console.warn(`nova: properties of ${path}: ${error}`);
            done(error ? null : result);
        });
    }

    // Opens another Nova Files window on `folder` (absolute).
    function openWindow(folder: string) {
        Daemon.call(Protocol.CMD_SHOW_FOLDER, {
            folder: folder
        }, error => {
            if (error)
                console.warn(`nova: a window on ${folder}: ${error}`);
        });
    }

    // Counts what `paths` (absolute, fitting one request) hold. Returns
    // the job's number, which its `measured` events carry;
    // `failed(job)` hears when novad refuses to start it.
    function measure(paths: var, failed: var): int {
        const job = newJob();
        Daemon.call(Protocol.CMD_FILES_MEASURE, {
            job: job,
            paths: paths
        }, error => {
            if (error) {
                console.warn(`nova: counting sizes: ${error}`);
                failed?.(job);
            }
        });
        return job;
    }

    // Stops measuring job `job`; it sends nothing more.
    function cancelMeasure(job: int) {
        Daemon.call(Protocol.CMD_FILES_MEASURE_CANCEL, {
            job: job
        });
    }

    // Sets who may read, change and run the item at `path` ({owner, group,
    // others}, each {read, write, run}). `done(error)` gets novad's answer.
    function setAccess(path: string, access: var, done: var) {
        Daemon.call(Protocol.CMD_FILES_SET_ACCESS, {
            path: path,
            access: access
        }, error => root._answered("changing access", error, done));
    }

    // Moves `paths` (absolute) to the trash, one step to undo however many
    // requests it takes. `done(error)` gets novad's answer: null, or which
    // ones didn't go.
    function trash(paths: var, done: var) {
        _inBatches(Protocol.CMD_FILES_TRASH, "paths", paths, "moving to the trash", done, true);
    }

    // Undoes the last copy, move or trashing done in any window (Ctrl+Z),
    // or with `redo` does the last one undone again (Ctrl+Y). A move back
    // runs as job `job` (newJob()), which reports like a transfer's;
    // `done(error, started)` gets novad's answer and whether that job
    // started.
    function undo(redo: bool, job: int, done: var) {
        Daemon.call(redo ? Protocol.CMD_FILES_REDO : Protocol.CMD_FILES_UNDO, {
            job: job
        }, (error, started) => {
            root._answered(redo ? "redoing" : "undoing", error, null);
            done(error ?? null, started === true);
        });
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
    // gets every request's errors at once, or null. With `oneStep`, the
    // batches after the first continue the first's step to undo.
    function _inBatches(cmd: string, key: string, items: var, doing: string, done: var, oneStep: bool) {
        const batches = Trash.batches(items, Protocol.REQUEST_MAX_BYTES);
        const errors = [];
        const next = index => {
            if (index === batches.length) {
                root._answered(doing, errors.length > 0 ? errors.join("\n") : null, done);
                return;
            }
            const args = {};
            args[key] = batches[index];
            if (oneStep && index > 0)
                args.continues = true;
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
        function onFilesMeasured(measured: var) {
            root.measured(measured);
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
