pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../files"
import "../theme"
import "../files/clipboard.js" as Clipboard
import "../files/commands.js" as Commands
import "../files/details.js" as Details
import "../files/listing.js" as Listing
import "../files/menus.js" as Menus
import "../daemon/protocol.js" as Protocol

// Nova Files' menus and what they lead to: the shortcuts each command is
// matched and shown by, the menus on items, the folder, a drive and the
// command row, the menu itself from the mouse and the keyboard, the
// clipboard's text, renaming in place, the details pane's words, and Open
// with's apps from a stand-in novad. The root is an Item: a TestCase root
// gets no mouse events.
Item {
    id: stage
    width: 880
    height: 700

    TestCase {
        name: "FilesMenus"
        when: windowShown

        function file(name) {
            return {
                name: name,
                dir: false,
                link: false,
                hidden: false,
                mime: "text/plain",
                size: 3,
                modified: 5
            };
        }

        function folder(name) {
            return {
                name: name,
                dir: true,
                link: false,
                hidden: false,
                mime: "inode/directory",
                size: -1,
                modified: 5
            };
        }

        function labels(menu) {
            return menu.map(item => item.separator ? "-" : item.label);
        }

        function test_the_item_menu_follows_what_is_picked() {
            const act = () => {};
            const one = Menus.forItems([file("a.txt")], () => [], () => "", act);
            compare(labels(one), ["Open", "Open with…", "-", "Rename…", "Cut", "Copy", "Copy path", "-", "Show details…", "-", "Move to trash"]);
            const two = Menus.forItems([file("a.txt"), folder("b")], () => [], () => "", act);
            compare(labels(two), ["Open", "-", "Cut", "Copy", "Copy path", "-", "Move to trash"], "nothing for one item only, no new window for a file");
            const folders = Menus.forItems([folder("a"), folder("b")], () => [], () => "", act);
            verify(labels(folders).includes("Open in new window"));
            compare(one.find(item => item.label === "Cut").shortcut, "Ctrl + X");
        }

        function test_menu_items_act_with_their_command_and_rows() {
            const asked = [];
            const act = (command, rows, extra) => asked.push([command, rows.map(row => row.name), extra]);
            const rows = [file("a.txt")];
            const apps = [
                {
                    id: "gedit.desktop",
                    name: "Text Editor"
                },
                {
                    id: "vim.desktop",
                    name: "Vim"
                }
            ];
            const menu = Menus.forItems(rows, () => apps, () => "vim.desktop", act);
            menu.find(item => item.label === "Copy").act();
            const openWith = menu.find(item => item.label === "Open with…").items();
            compare(openWith.map(item => [item.label, item.checked]), [["Text Editor", false], ["Vim", true]]);
            openWith[0].act();
            compare(asked, [[Commands.COPY, ["a.txt"], undefined], [Commands.OPEN_WITH, ["a.txt"], "gedit.desktop"]]);
        }

        function test_the_folder_drive_and_sort_menus() {
            const act = () => {};
            const empty = Menus.forFolder(false, true, act);
            compare(labels(empty), ["New folder", "Paste", "-", "Select all", "Show hidden files", "-", "Show details…"]);
            compare(empty[1].enabled, false, "nothing to paste");
            compare(empty[4].checked, true);
            verify(Menus.forFolder(true, false, act)[1].enabled);
            compare(labels(Menus.forDrive({
                removable: false
            }, act)), ["Open in new window", "Show details…"], "no Eject for a disk inside");
            verify(labels(Menus.forDrive({
                removable: true
            }, act)).includes("Eject"));
            const picked = [];
            const sort = Menus.forSort({
                key: Protocol.FILE_SORT_SIZE,
                descending: true
            }, (command, rows, order) => picked.push(order));
            compare(sort.filter(item => item.checked).map(item => item.label), ["Size", "Descending"]);
            sort[0].act();
            sort[sort.length - 2].act();
            compare(picked, [
                {
                    key: Protocol.FILE_SORT_NAME,
                    descending: true
                },
                {
                    key: Protocol.FILE_SORT_SIZE,
                    descending: false
                }
            ]);
        }

        function test_the_clipboard_holds_file_uris_and_takes_paths_back() {
            const paths = ["/home/ana/My Photos/a#1.png", "/home/ana/é.txt"];
            const text = Clipboard.text(paths);
            compare(text, "file:///home/ana/My%20Photos/a%231.png\nfile:///home/ana/%C3%A9.txt");
            compare(Clipboard.paths(text), paths);
            compare(Clipboard.paths("/\n/home/ana"), [], "a path copied as text isn't files to copy");
            compare(Clipboard.paths("file://localhost/tmp/x"), ["/tmp/x"]);
            compare(Clipboard.paths("some words"), [], "text that isn't files pastes nothing");
            compare(Clipboard.paths("file:///a\nhello"), [], "all or nothing");
            compare(Clipboard.paths("file://other/tmp/x"), [], "another machine's file");
            compare(Clipboard.paths(""), []);
        }

        function test_renaming_picks_the_name_before_its_extension() {
            compare(Listing.stemLength("photo.final.png", false), 11);
            compare(Listing.stemLength("photo.png", true), 9, "a folder's name whole");
            compare(Listing.stemLength(".bashrc", false), 7, "a dot file's whole");
            compare(Listing.stemLength("README", false), 6);
        }

        function test_details_read_like_cosmic_files() {
            const details = {
                entry: {
                    name: "Trips",
                    dir: true,
                    link: false,
                    hidden: false,
                    mime: "inode/directory",
                    modified_ms: Date.UTC(2026, 9, 29, 12, 42)
                },
                items: 1,
                owner: "ana",
                group: "users",
                mode: 0o750
            };
            compare(Details.facts(details).map(fact => fact.label), ["Items", "Modified"], "no times the file system didn't keep");
            compare(Details.facts(details)[0].value, "1");
            compare(Details.access(details).map(line => [line.who, line.role, line.can]), [["ana", "Owner", "Read, write, and execute"], ["users", "Group", "Read and execute"], ["Others", "", "None"]]);
            compare([Details.can(6), Details.can(4), Details.can(2), Details.can(1), Details.can(3)], ["Read and write", "Read-only", "Write-only", "Execute-only", "Write and execute"], "execute is never dropped");
            const notes = {
                entry: {
                    name: "notes.txt",
                    dir: false,
                    link: false,
                    hidden: false,
                    mime: "text/plain",
                    size: 2048
                },
                owner: "ana",
                group: "users",
                mode: 0o644
            };
            compare(Details.facts(notes).map(fact => fact.label), ["Size"]);
        }

        Component {
            id: menuComponent
            FilesMenu {
                width: stage.width
                height: stage.height
            }
        }

        SignalSpy {
            id: closes
            signalName: "closed"
        }

        function test_the_menu_picks_with_the_mouse_and_the_keys() {
            const menu = createTemporaryObject(menuComponent, stage);
            closes.target = menu;
            closes.clear();
            const picked = [];
            const items = [
                {
                    label: "Open",
                    act: () => picked.push("open")
                },
                Menus.SEPARATOR,
                {
                    label: "Paste",
                    enabled: false,
                    act: () => picked.push("paste")
                },
                {
                    label: "Copy",
                    act: () => picked.push("copy")
                }
            ];
            menu.open(items, Qt.point(stage.width - 10, 20));
            verify(menu.shown);
            waitForRendering(menu);
            const open = findText(menu, "Open");
            verify(open.mapToItem(stage, open.width, 0).x <= stage.width, "kept inside the window");
            mouseClick(open);
            compare(picked, ["open"]);
            compare([menu.shown, closes.count], [false, 1]);

            menu.open(items, Qt.point(20, 20));
            keyClick(Qt.Key_Down);
            keyClick(Qt.Key_Down);
            keyClick(Qt.Key_Return);
            compare(picked, ["open", "copy"], "the separator and Paste, which can't, are skipped");
            menu.open(items, Qt.point(20, 20));
            // The rows are laid out as it's drawn.
            waitForRendering(menu);
            mouseClick(findText(menu, "Paste"));
            verify(menu.shown, "Paste can't, so the menu stays");
            keyClick(Qt.Key_Escape);
            compare([menu.shown, closes.count], [false, 3]);
        }

        function test_a_long_menu_scrolls_inside_the_window() {
            const menu = createTemporaryObject(menuComponent, stage);
            const items = [];
            for (let index = 0; index < 40; index++)
                items.push({
                    label: `App ${index}`,
                    act: () => {}
                });
            menu.open(items, Qt.point(20, 20));
            waitForRendering(menu);
            const last = findText(menu, "App 39");
            keyClick(Qt.Key_Up);
            waitForRendering(menu);
            const shown = last.mapToItem(stage, 0, 0).y;
            verify(shown >= 0 && shown + last.height <= stage.height, "the row picked from the keyboard is in view");
        }

        function test_a_submenu_opens_beside_its_row() {
            const menu = createTemporaryObject(menuComponent, stage);
            const picked = [];
            menu.open([
                {
                    label: "Open with…",
                    items: () => [
                            {
                                label: "Vim",
                                act: () => picked.push("vim")
                            }
                        ]
                }
            ], Qt.point(20, 20));
            keyClick(Qt.Key_Down);
            keyClick(Qt.Key_Right);
            const vim = findText(menu, "Vim");
            verify(vim, "shown");
            verify(vim.mapToItem(menu, 0, 0).x > Tokens.filesMenuWidth, "beside it");
            keyClick(Qt.Key_Return);
            compare(picked, ["vim"]);
        }

        Component {
            id: rowComponent
            FileRow {
                width: 600
                index: 0
                name: "photo.png"
                dir: false
                hidden: false
                mime: "image/png"
                size: 1
                modified: 5
                nameWidth: 300
                columnWidths: Tokens.filesColumns
                selected: true
                riseOrder: -1
                now: 5
            }
        }

        SignalSpy {
            id: renames
            signalName: "renameDone"
        }

        function test_a_name_field_answers_once() {
            const row = createTemporaryObject(rowComponent, stage);
            renames.target = row;
            renames.clear();
            row.renaming = true;
            waitForRendering(row);
            keyClick(Qt.Key_A);
            keyClick(Qt.Key_Return);
            compare(renames.count, 1);
            compare(renames.signalArguments[0][0], "a.png", "the name up to its extension was picked");
            row.renaming = false;
            row.renaming = true;
            keyClick(Qt.Key_Down);
            keyClick(Qt.Key_F2);
            compare(renames.count, 1, "other keys stay in the field");
            row.renaming = false;
            row.renaming = true;
            keyClick(Qt.Key_Escape);
            compare(renames.signalArguments[1][0], "", "Esc keeps the name");
            row.renaming = false;
            compare(renames.count, 2, "closing it asks nothing more");
        }

        // Stands in for FilesService: answers when the test says.
        QtObject {
            id: service

            property var asked: []
            property var defaults: []

            function openWith(path, done) {
                asked.push({
                    path: path,
                    done: done
                });
            }
            function setDefault(path, app, done) {
                defaults.push(app);
                done(null);
            }
            function launch(path, app, done) {
            }
        }

        Component {
            id: openWithComponent
            OpenWithModel {}
        }

        function test_open_with_keeps_only_the_latest_answer() {
            service.asked = [];
            const model = createTemporaryObject(openWithComponent, stage, {
                service: service,
                path: "/home/ana/a.png"
            });
            verify(model.loading);
            model.path = "/home/ana/b.txt";
            compare(service.asked.map(ask => ask.path).slice(-2), ["/home/ana/a.png", "/home/ana/b.txt"]);
            const late = service.asked[service.asked.length - 2];
            const latest = service.asked[service.asked.length - 1];
            latest.done({
                mime: "text/plain",
                default: "vim.desktop",
                apps: [
                    {
                        id: "vim.desktop",
                        name: "Vim"
                    }
                ]
            });
            late.done({
                mime: "image/png",
                apps: []
            });
            compare([model.mime, model.defaultApp, model.apps.length, model.loading], ["text/plain", "vim.desktop", 1, false], "a.png's late answer is dropped");
            const before = service.asked.length;
            model.makeDefault("gedit.desktop");
            compare(service.defaults, ["gedit.desktop"]);
            compare(service.asked.length, before + 1, "the apps are read again");
            verify(model.loading);
        }

        function findText(item, text) {
            for (const child of item.children) {
                if (child instanceof Text && child.visible && child.text === text)
                    return child;
                const found = findText(child, text);
                if (found)
                    return found;
            }
            return null;
        }
    }
}
