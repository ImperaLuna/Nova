import QtQml
import "../daemon/protocol.js" as Protocol
import "navigation.js" as Navigation
import "paths.js" as Paths
import "home.js" as Home
import "service"

// Where a Nova Files window is and how it moves: go to a folder (a click
// in the sidebar or the address bar, or a typed path), back, forward and
// up through its history, and the sidebar's tree for where it is. Each
// move asks novad for the tree on the new folder first (files_tree) and
// moves only once that answers, so a folder that's gone or a mistyped
// path leaves the window where it was. An answer to an older move than
// the latest is dropped. The Home page and the trash (Home.isPage) are
// places in the history like folders, with nothing above them; while the
// window is on one, the tree stays on the last folder it showed, or Home's
// folder for a window that opened on one.
QtObject {
    id: navigator

    // The folder shown; "" until begin().
    property string folder: ""
    // Hidden files show, here and in the tree.
    property bool hidden: false
    property var history: Navigation.start("")
    readonly property bool canGoBack: history.at > 0
    readonly property bool canGoForward: history.at < history.entries.length - 1
    readonly property bool canGoUp: folder !== "" && !Home.isPage(folder) && Paths.parent(folder) !== ""
    // The tree's rows (files_tree), and the folders the user opened or
    // closed in it.
    property var treeRows: []
    property var treeKept: ({
            opened: [],
            closed: []
        })
    property int moves: 0
    // The folder the tree was last worked out for.
    property string treeFolder: ""

    // Starts at `start` (absolute), with nothing to go back to.
    function begin(start: string) {
        resume(Navigation.start(start));
    }

    // Starts where `kept` (a history another window's tab had) is, with
    // its back and forward.
    function resume(kept: var) {
        folder = Navigation.current(kept);
        history = kept;
        go(folder, "stay", 0, null);
    }

    // Goes to `path` (absolute, or from `~`); `done(ok)`, when given, says
    // whether it did.
    function visit(path: string, done: var) {
        go(path, "visit", 0, done);
    }

    // Moves `steps` back (negative) or forward through the history.
    function step(steps: int) {
        const moved = Navigation.step(history, steps);
        if (moved)
            go(Navigation.current(moved), "step", steps, null);
    }

    // Mounts the drive `id` unless it is, then goes there; stays put when
    // it can't be mounted.
    function openDrive(id: string) {
        FilesService.mount(id, mounted => {
            if (mounted !== null)
                navigator.visit(mounted, null);
        });
    }

    function up() {
        if (canGoUp)
            visit(Paths.parent(folder), null);
    }

    // Opens or closes the tree's row at `path`, which is `open` now.
    function toggle(path: string, open: bool) {
        treeKept = Navigation.toggle(treeKept, path, open, Protocol.FILES_TREE_MAX_FOLDERS);
        go(folder, "stay", 0, null);
    }

    // `how` is "visit" (a new place in the history), "step" (back or
    // forward, `steps` of them) or "stay" (the tree changed, not the folder).
    function go(path: string, how: string, steps: int, done: var) {
        const move = ++moves;
        const leaving = how !== "stay";
        const page = Home.isPage(path);
        if (page && leaving) {
            arrive(how, steps, path, treeRows);
            done?.(true);
            return;
        }
        // On a page only the tree changes, on its last folder.
        const asked = page ? (treeFolder !== "" ? treeFolder : "~") : path;
        FilesService.tree(asked, treeKept.opened, leaving ? [] : treeKept.closed, hidden, (error, tree) => {
            if (move !== moves)
                return;
            if (error) {
                console.warn(`nova: files: ${asked}: ${error}`);
                done?.(false);
                return;
            }
            treeFolder = tree.folder;
            arrive(how, steps, page ? path : tree.folder, tree.rows);
            done?.(true);
        });
    }

    // Shows `place` (a folder or a page) with the tree's `rows`.
    function arrive(how: string, steps: int, place: string, rows: var) {
        if (how === "visit")
            history = Navigation.visit(history, place);
        else if (how === "step")
            history = Navigation.step(history, steps);
        if (how !== "stay")
            treeKept = {
                opened: treeKept.opened,
                closed: []
            };
        folder = place;
        treeRows = rows;
    }

    onHiddenChanged: {
        if (folder !== "")
            go(folder, "stay", 0, null);
    }
}
