pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol
import "../trash.js" as Trash
import "../thumbnails.js" as Thumbnails

// Nova Files' link to novad: the windows it asks for, the folders they
// show (folder views: each FolderModel opens one under its own number and
// gets its pages, changes and closing here), the sidebar's tree and
// places (the Home page, Quick access and the trash, read again whenever
// a folder is pinned or unpinned), recent files for the Home page, the
// drives (and mounting and ejecting them), opening files in
// apps (the default one on a double-click or Enter, or one from "Open
// with", which can also become the new default), copy and move jobs, the
// trash, the Properties window's facts, sizes and access, the preview
// pane's previews, thumbnails for the Icons view, finding files by name,
// and answering the file pickers apps ask for (Open and Save dialogs). The
// Nova Files process (../../files.qml) and its windows get these here and
// never talk to Daemon themselves. Also how solid the windows are, from
// config.toml's [files].
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
    // The sidebar's places, in its order ({kind, path, name}): the Home
    // page, the folders pinned to Quick access, the trash. Empty until
    // novad answers.
    property var places: []
    // Which read `places` shows, so a late answer to an earlier one is
    // dropped.
    property int _readingPlaces: 0

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
    // `sort` ({key: a Protocol.FILE_SORT_*, descending, mixed: folders
    // and files together}), with dot files when `hidden`, and only the
    // files `filter` matches when it isn't null (a file picker's {name,
    // globs, mimes}). Whatever the view showed is replaced; its entries
    // come as `page`s, and folders' sizes as `changed` once novad has
    // counted them. `done(error)` gets novad's answer.
    function open(view: int, path: string, sort: var, hidden: bool, filter: var, done: var) {
        const args = {
            view: view,
            path: path,
            sort: {
                key: sort.key,
                descending: sort.descending,
                mixed: sort.mixed ?? false
            },
            hidden: hidden
        };
        if (filter)
            args.filter = filter;
        Daemon.call(Protocol.CMD_FILES_OPEN, args, error => {
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
    // novad's index of file names is built: a search that found nothing
    // because it wasn't can be asked again.
    signal searchReady

    // Paths whose thumbnails were asked for, each to the callbacks waiting
    // for it; a map without a prototype, so any path is a key.
    property var _thumbnailsWaiting: Object.create(null)

    // The file picker `picker`'s app stopped asking: its window closes.
    signal pickerClosed(int picker)

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

    // `done(preview)` gets what the preview pane shows of the file at
    // `path` (absolute): {kind: Protocol.PREVIEW_IMAGE, path} (a picture to
    // load), {kind: Protocol.PREVIEW_TEXT, text, cut} (its first lines,
    // `cut` when it goes on) or {kind: Protocol.PREVIEW_NONE}; false when
    // novad couldn't tell.
    function preview(path: string, done: var) {
        Daemon.call(Protocol.CMD_FILES_PREVIEW, {
            path: path
        }, (error, result) => {
            if (error)
                console.warn(`nova: previewing ${path}: ${error}`);
            done(error ? false : result);
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

    // Reads `places` again; a failed read keeps what's there.
    function refreshPlaces() {
        const reading = ++_readingPlaces;
        Daemon.call(Protocol.CMD_FILES_PLACES, undefined, (error, result) => {
            // An answer to an earlier read, finishing late, is dropped.
            if (reading !== root._readingPlaces)
                return;
            if (error)
                console.warn(`nova: sidebar places: ${error}`);
            else
                root.places = result;
        });
    }

    // Pins the folder at `path` (absolute) to Quick access, or unpins it;
    // `places` follows once novad says it changed.
    function pin(path: string, pinned: bool) {
        Daemon.call(Protocol.CMD_FILES_PIN, {
            path: path,
            pinned: pinned
        }, error => {
            if (error)
                console.warn(`nova: ${pinned ? "pinning" : "unpinning"} ${path}: ${error}`);
        });
    }

    // `done(files)` gets the files used last, by any app, the latest first,
    // each {folder, used_ms, entry}; null when novad couldn't tell.
    function recent(done: var) {
        Daemon.call(Protocol.CMD_FILES_RECENT, undefined, (error, result) => {
            if (error)
                console.warn(`nova: recent files: ${error}`);
            done(error ? null : result);
        });
    }

    // Finds files by name: `done(found)` gets novad's answer ({ready,
    // total, results}, see Protocol's FileSearch) for `query`, below the
    // folder `within` (absolute) or everywhere when it's "", or null when
    // novad couldn't search.
    function search(query: string, within: string, done: var) {
        const args = within !== "" ? {
            query: query,
            within: within
        } : {
            query: query
        };
        Daemon.call(Protocol.CMD_FILES_SEARCH, args, (error, result) => {
            if (error)
                console.warn(`nova: searching for ${query}: ${error}`);
            done(error ? null : result);
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

    // Counts what `paths` (absolute) hold, in as many jobs as keep each
    // request under novad's limit. Returns the jobs' numbers, whose
    // `measured` events carry them.
    function measure(paths: var): var {
        return Trash.batches(paths, Protocol.REQUEST_MAX_BYTES).map(batch => {
            const job = _nextJob++;
            Daemon.call(Protocol.CMD_FILES_MEASURE, {
                job: job,
                paths: batch
            }, error => {
                if (error)
                    console.warn(`nova: counting sizes: ${error}`);
            });
            return job;
        });
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

    // `done(games)` gets the installed games in name order, each {name,
    // folder, size, saves}; none when novad couldn't tell.
    function gamesList(done: var) {
        Daemon.call(Protocol.CMD_FILES_GAMES_LIST, undefined, (error, result) => {
            if (error)
                console.warn(`nova: games: ${error}`);
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

    // `done(file)` gets the path of a PNG thumbnail of the file at `path`
    // (absolute) as it was at `modified` (its listing's time, ms), or ""
    // when it has none. Asking for a path already asked for waits for the
    // same answer; an answer for another version of the file asks once
    // more for this one.
    function thumbnail(path: string, modified: real, done: var) {
        const wanted = {
            modified: modified,
            done: done,
            again: false
        };
        const waiting = _thumbnailsWaiting[path];
        if (waiting) {
            waiting.push(wanted);
            return;
        }
        _thumbnailsWaiting[path] = [wanted];
        _askThumbnail(path);
    }

    // `done` no longer wants `path`'s thumbnail; once nothing does, novad
    // drops it unless it's being made.
    function cancelThumbnail(path: string, done: var) {
        const waiting = _thumbnailsWaiting[path];
        if (!waiting)
            return;
        const left = waiting.filter(wanted => wanted.done !== done);
        if (left.length > 0) {
            _thumbnailsWaiting[path] = left;
            return;
        }
        delete _thumbnailsWaiting[path];
        Daemon.call(Protocol.CMD_FILES_THUMBNAIL_CANCEL, {
            path: path
        });
    }

    function _askThumbnail(path: string) {
        Daemon.call(Protocol.CMD_FILES_THUMBNAIL, {
            path: path
        }, error => {
            if (error)
                root._thumbnailAnswered({
                    path: path
                });
        });
    }

    // novad's answer ({path, modified_ms, thumbnail}) goes to those who
    // wanted that version of the file; the others ask once more
    // (Thumbnails.split).
    function _thumbnailAnswered(answer: var) {
        const waiting = _thumbnailsWaiting[answer.path];
        if (!waiting)
            return;
        const {
            answered,
            again
        } = Thumbnails.split(waiting, answer);
        if (again.length > 0) {
            for (const wanted of again)
                wanted.again = true;
            _thumbnailsWaiting[answer.path] = again;
            _askThumbnail(answer.path);
        } else {
            delete _thumbnailsWaiting[answer.path];
        }
        for (const wanted of answered)
            wanted.done(answer.thumbnail ?? "");
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

    // Answers the file picker `picker` with `paths` (absolute), `filter`
    // being the index of the filter shown (or -1), over a file that's
    // there only when `replace`. `done(error)` gets novad's answer: null,
    // Protocol.FILES_PICK_EXISTS for a save that would replace a file, or
    // why the paths don't fit the picker. A selection too long for one
    // request isn't sent: novad would close the connection, and with it
    // every picker it shows.
    function pick(picker: int, paths: var, filter: int, replace: bool, done: var) {
        if (Trash.batches(paths, Protocol.REQUEST_MAX_BYTES).length > 1) {
            done(qsTr("Too many items are selected to answer at once. Select fewer."));
            return;
        }
        const args = {
            picker: picker,
            paths: paths,
            replace: replace
        };
        if (filter >= 0)
            args.filter = filter;
        Daemon.call(Protocol.CMD_FILES_PICK, args, error => {
            if (error && error !== Protocol.FILES_PICK_EXISTS)
                console.warn(`nova: answering the file picker: ${error}`);
            done(error ?? null);
        });
    }

    // The file picker `picker` closed without picking.
    function cancelPick(picker: int) {
        Daemon.call(Protocol.CMD_FILES_PICK_CANCEL, {
            picker: picker
        });
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
        function onFilesSearchReady() {
            root.searchReady();
        }
        function onFilesTrashChanged() {
            root.trashChanged();
        }
        function onFilesThumbnail(answer: var) {
            root._thumbnailAnswered(answer);
        }
        function onFilesPlacesChanged() {
            root.refreshPlaces();
        }
        function onFilesPickerClosed(picker: int) {
            root.pickerClosed(picker);
        }
        function onConnectedChanged() {
            if (Daemon.connected) {
                root.attach();
                root.refreshPlaces();
                // novad forgot what the last connection asked for.
                for (const path of Object.keys(root._thumbnailsWaiting))
                    root._askThumbnail(path);
            } else {
                root.transfersLost();
            }
        }
    }
}
