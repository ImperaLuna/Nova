import QtQuick
import QtTest
import "../files/listing.js" as Listing
import "../files/paths.js" as Paths
import "../files/navigation.js" as Navigation
import "../daemon/protocol.js" as Protocol
import "../theme/icons.js" as Icons

// Nova Files' address bar parts, window title and entry paths, its
// history and the sidebar tree's kept folders and rows, and its file list:
// how an entry reads in the columns, sorting by a header, what clicks and
// keys select, and what opening the selection does.
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
        Navigation.sync(model, ["/a", "/b/x", "/b"].map(row));
        compare(model.count, 3, "a row that moved isn't added twice");
        compare(model.get(1).path, "/b/x");
        Navigation.sync(model, []);
        compare(model.count, 0);
    }

    function test_an_entry_path_joins_the_folder() {
        compare(Paths.child("/home/ana", "notes.md"), "/home/ana/notes.md");
        compare(Paths.child("/", "etc"), "/etc");
    }

    function entry(name, fields) {
        return Listing.row(Object.assign({
            name: name,
            dir: false,
            link: false,
            hidden: name.startsWith("."),
            mime: "application/octet-stream"
        }, fields));
    }

    function test_a_row_has_every_role() {
        const row = entry("Music", {
            dir: true,
            mime: "inode/directory"
        });
        compare(row.size, -1, "a folder has no size");
        compare(row.modified, -1);
        compare(entry("a.png", {
            size: 48213,
            modified_ms: 1793000100000
        }).modified, 1793000100000);
    }

    function test_icons_follow_the_kind() {
        compare(Listing.icon(entry("Music", {
            dir: true,
            mime: "inode/directory"
        })), Icons.FOLDER);
        compare(Listing.icon(entry("a.png", {
            mime: "image/png"
        })), Icons.PICTURE);
        compare(Listing.icon(entry("a.mkv", {
            mime: "video/x-matroska"
        })), Icons.VIDEO);
        compare(Listing.icon(entry("a.pdf", {
            mime: "application/pdf"
        })), Icons.DOCUMENT);
        compare(Listing.icon(entry("a.bin", {})), Icons.FILE);
    }

    function test_the_type_column_reads_short() {
        const kind = mime => Listing.kind(entry("x", {
                mime: mime
            }));
        compare(Listing.kind(entry("Music", {
            dir: true,
            mime: "inode/directory"
        })), "Folder");
        compare(kind("application/pdf"), "PDF");
        compare(kind("image/svg+xml"), "SVG");
        compare(kind("video/x-matroska"), "Matroska");
        compare(kind("text/markdown"), "Markdown");
        compare(kind("text/plain"), "Text");
        compare(kind("application/octet-stream"), "File");
    }

    function test_modified_reads_today_this_year_or_with_the_year() {
        const now = new Date(2026, 10, 2, 18, 30).getTime();
        compare(Listing.modified(new Date(2026, 10, 2, 16, 12).getTime(), now), "Today 16:12");
        compare(Listing.modified(new Date(2026, 10, 1, 9, 0).getTime(), now), Qt.formatDate(new Date(2026, 10, 1), "d MMM"));
        compare(Listing.modified(new Date(2025, 6, 13).getTime(), now), Qt.formatDate(new Date(2025, 6, 13), "d MMM yyyy"));
        compare(Listing.modified(-1, now), "", "novad didn't say");
    }

    function test_sizes_and_counts() {
        compare(Listing.size(entry("a.pdf", {
            size: 240 * 1024
        })), "240 KB");
        compare(Listing.size(entry("Music", {
            dir: true,
            mime: "inode/directory"
        })), "");
        compare(Listing.items(1), "1 item");
        compare(Listing.items(5), "5 items");
    }

    function test_a_header_click_sorts_by_its_column() {
        const byName = {
            key: Protocol.FILE_SORT_NAME,
            descending: false
        };
        compare(Listing.sorted(byName, Protocol.FILE_SORT_NAME), {
            key: Protocol.FILE_SORT_NAME,
            descending: true
        }, "the same column turns the order round");
        compare(Listing.sorted({
            key: Protocol.FILE_SORT_SIZE,
            descending: true
        }, Protocol.FILE_SORT_MODIFIED), {
            key: Protocol.FILE_SORT_MODIFIED,
            descending: false
        });
    }

    function test_a_remembered_row_follows_its_entry() {
        compare(Listing.shifted(3, {
            op: Protocol.FOLDER_INSERT,
            index: 1
        }), 4);
        compare(Listing.shifted(3, {
            op: Protocol.FOLDER_INSERT,
            index: 3
        }), 4, "inserted at its place: it moves down");
        compare(Listing.shifted(3, {
            op: Protocol.FOLDER_INSERT,
            index: 5
        }), 3);
        compare(Listing.shifted(3, {
            op: Protocol.FOLDER_REMOVE,
            index: 0
        }), 2);
        compare(Listing.shifted(3, {
            op: Protocol.FOLDER_REMOVE,
            index: 3
        }), 3, "removed: the next row takes its place");
        compare(Listing.shifted(3, {
            op: Protocol.FOLDER_UPDATE,
            index: 3
        }), 3);
        compare(Listing.shifted(-1, {
            op: Protocol.FOLDER_INSERT,
            index: 0
        }), -1, "no row remembered");
    }

    function test_clicks_select_like_windows() {
        compare(Listing.pick(-1, 4, false, false), {
            clear: true,
            from: 4,
            to: 4,
            toggle: false,
            anchor: 4
        }, "a plain click selects only that row");
        compare(Listing.pick(4, 2, true, false), {
            clear: false,
            from: 2,
            to: 2,
            toggle: true,
            anchor: 2
        }, "Ctrl flips one row");
        compare(Listing.pick(2, 6, false, true), {
            clear: true,
            from: 2,
            to: 6,
            toggle: false,
            anchor: 2
        }, "Shift selects the run from the anchor");
        compare(Listing.pick(6, 2, true, true), {
            clear: false,
            from: 2,
            to: 6,
            toggle: false,
            anchor: 6
        }, "Ctrl+Shift adds the run");
        compare(Listing.pick(-1, 3, false, true).anchor, 3, "Shift with no anchor is a plain click");
    }

    function test_keys_move_within_the_rows() {
        compare(Listing.moved(Qt.Key_Down, 2, 10, 4), 3);
        compare(Listing.moved(Qt.Key_Down, 9, 10, 4), 9);
        compare(Listing.moved(Qt.Key_Up, 0, 10, 4), 0);
        compare(Listing.moved(Qt.Key_Down, -1, 10, 4), 0, "nothing current: the first row");
        compare(Listing.moved(Qt.Key_Up, -1, 10, 4), 0);
        compare(Listing.moved(Qt.Key_PageDown, 8, 10, 4), 9);
        compare(Listing.moved(Qt.Key_PageUp, 6, 10, 4), 2);
        compare(Listing.moved(Qt.Key_Home, 6, 10, 4), 0);
        compare(Listing.moved(Qt.Key_End, 0, 10, 4), 9);
        verify(Listing.MOVES.includes(Qt.Key_End));
        verify(!Listing.MOVES.includes(Qt.Key_Return));
    }

    function test_opening_goes_into_one_folder_or_opens_files() {
        const music = entry("Music", {
            dir: true,
            mime: "inode/directory"
        });
        const notes = entry("notes.md", {
            mime: "text/markdown"
        });
        compare(Listing.opening([music]), {
            folder: "Music",
            files: []
        });
        compare(Listing.opening([music, notes]), {
            folder: "",
            files: ["notes.md"]
        }, "folders among files stay put");
        compare(Listing.opening([]), {
            folder: "",
            files: []
        });
    }
}
