import QtQuick
import QtTest
import "../files/paths.js" as Paths
import "../files/navigation.js" as Navigation

// Nova Files' address bar parts and window title from a folder's path,
// its history, and the sidebar tree's kept folders and rows.
TestCase {
    name: "Files"

    function names(parts) {
        return parts.map(part => part.name);
    }

    function test_a_folder_in_home_starts_at_home() {
        const parts = Paths.crumbs("/home/ana/.local/share/Steam", "/home/ana", "Home");
        compare(names(parts), ["Home", ".local", "share", "Steam"]);
        compare(parts.map(part => part.path), ["/home/ana", "/home/ana/.local", "/home/ana/.local/share", "/home/ana/.local/share/Steam"]);
        compare(parts.map(part => part.hidden), [false, true, true, true], "inside a dot folder is hidden too");
        compare(names(Paths.crumbs("/home/ana", "/home/ana", "Home")), ["Home"]);
    }

    function test_a_folder_outside_home_starts_at_the_root() {
        compare(names(Paths.crumbs("/home/anabel/Music", "/home/ana", "Home")), ["/", "home", "anabel", "Music"], "only home itself, not a name it starts");
        compare(names(Paths.crumbs("/mnt/usb", "", "Home")), ["/", "mnt", "usb"]);
        compare(names(Paths.crumbs("/", "/home/ana", "Home")), ["/"]);
    }

    function test_the_title_is_the_folder_name() {
        compare(Paths.folderName("/home/ana/Music"), "Music");
        compare(Paths.folderName("/"), "/");
    }

    function test_up_is_the_folder_holding_it() {
        compare(Paths.parent("/home/ana/Music"), "/home/ana");
        compare(Paths.parent("/home"), "/");
        compare(Paths.parent("/"), "");
    }

    function test_history_goes_back_and_forward_like_a_browser() {
        let history = Navigation.start("/a");
        history = Navigation.visit(history, "/a/b");
        history = Navigation.visit(history, "/a/b/c");
        compare(Navigation.visit(history, "/a/b/c"), history, "going where it is changes nothing");
        history = Navigation.step(history, -2);
        compare(Navigation.current(history), "/a");
        compare(Navigation.step(history, -1), null, "nothing before the start");
        compare(Navigation.current(Navigation.step(history, 1)), "/a/b");
        history = Navigation.visit(history, "/x");
        compare(history.entries, ["/a", "/x"], "a new place drops what was ahead");
        compare(Navigation.step(history, 1), null);
    }

    function test_a_chevron_opens_or_closes_a_folder() {
        let kept = {
            opened: [],
            closed: []
        };
        kept = Navigation.toggle(kept, "/a", false, 2);
        compare(kept, {
            opened: ["/a"],
            closed: []
        });
        kept = Navigation.toggle(kept, "/a", true, 2);
        compare(kept, {
            opened: [],
            closed: ["/a"]
        }, "closing one on the way keeps it closed");
        kept = Navigation.toggle(kept, "/b", false, 2);
        kept = Navigation.toggle(kept, "/c", false, 2);
        kept = Navigation.toggle(kept, "/d", false, 2);
        compare(kept.opened, ["/c", "/d"], "only the newest are kept");
    }

    ListModel {
        id: model
    }

    function row(path) {
        return {
            name: path,
            path: path,
            depth: 0,
            open: false,
            leaf: true,
            hidden: false
        };
    }

    function test_tree_rows_change_only_where_they_differ() {
        Navigation.sync(model, ["/a", "/b", "/c"].map(row));
        const kept = model.get(1);
        let open = row("/b");
        open.open = true;
        Navigation.sync(model, [row("/a"), open, row("/b/x"), row("/b/y")]);
        const paths = [];
        for (let i = 0; i < model.count; i++)
            paths.push(model.get(i).path);
        compare(paths, ["/a", "/b", "/b/x", "/b/y"]);
        compare(model.get(1).open, true);
        Navigation.sync(model, []);
        compare(model.count, 0);
    }
}
