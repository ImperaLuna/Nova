pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../files"
import "../theme"
import "../files/listing.js" as Listing
import "../files/paths.js" as Paths
import "../files/navigation.js" as Navigation
import "../files/trash.js" as Trash
import "../files/home.js" as Home
import "../daemon/protocol.js" as Protocol
import "../theme/icons.js" as Icons
import "../files/transfers.js" as Transfers
import "../files/space.js" as Space
import "../files/search.js" as Search
import "../files/tabs.js" as Tabs
import "../files/preview.js" as Preview
import "../files/picker.js" as Picker

// Nova Files' address bar parts, window title and entry paths, its
// history and the sidebar tree's kept folders and rows, its file list:
// how an entry reads in the columns, sorting by a header, what clicks and
// keys select, and what opening the selection does, and the copy and move
// jobs a window keeps and how their card and name clash dialog read and
// answer, and the trash: its rows, its selection, and the dialog that asks
// before deleting for good, search: how results read and when novad
// is asked, a window as an app's Open or Save dialog: its title and
// accept label, what accepting picks, and its bar, what the preview pane shows, and the window's
// tabs: which one Ctrl+Tab and closing land on, their widths, dragging one
// along or out of the row, and the row's clicks. The root is an Item: a
// TestCase root gets no mouse events.
Item {
    id: stage
    width: 880
    height: 700

    TestCase {
        name: "Files"
        when: windowShown

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

        function test_a_folder_is_inside_a_mount_but_not_a_lookalike() {
            verify(Paths.inside("/run/media/ana/SANDISK/Photos", "/run/media/ana/SANDISK"));
            verify(Paths.inside("/run/media/ana/SANDISK", "/run/media/ana/SANDISK"));
            verify(!Paths.inside("/run/media/ana/SANDISK2", "/run/media/ana/SANDISK"), "a name it starts isn't inside");
            verify(Paths.inside("/home/ana", "/"), "everything is on the root");
            verify(!Paths.inside("/home/ana", ""), "an unmounted drive holds nothing");
        }

        function test_a_folder_outside_home_starts_at_the_root() {
            compare(names(Paths.crumbs("/home/anabel/Music", "/home/ana", "Home")), ["/", "home", "anabel", "Music"], "only home itself, not a name it starts");
            compare(names(Paths.crumbs("/mnt/usb", "", "Home")), ["/", "mnt", "usb"]);
            compare(names(Paths.crumbs("/", "/home/ana", "Home")), ["/"]);
        }

        // Names that aren't UTF-8 show a replacement character per run of raw
        // bytes, and keep their raw form in paths.
        function test_raw_bytes_show_as_one_replacement_character() {
            const eAcute = String.fromCodePoint(Protocol.RAW_BYTE_BASE + 0xE9);
            const grave = String.fromCodePoint(Protocol.RAW_BYTE_BASE + 0xE8);
            compare(Paths.shown(`caf${eAcute}.md`), "caf\uFFFD.md");
            compare(Paths.shown(`${eAcute}${grave}t${eAcute}`), "\uFFFDt\uFFFD");
            compare(Paths.shown("café ü"), "café ü");
            const parts = Paths.crumbs(`/mnt/M${eAcute}sik`, "", "Home");
            compare(names(parts), ["/", "mnt", "M\uFFFDsik"]);
            compare(parts[2].path, `/mnt/M${eAcute}sik`);
            compare(Paths.folderName(`/mnt/M${eAcute}sik`), "M\uFFFDsik");
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
            compare(row.archive, false, "only novad says what's an archive");
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
            })), "", "not counted yet");
            compare(Listing.size(entry("Music", {
                dir: true,
                mime: "inode/directory",
                size: 3 * 1024 * 1024
            })), "3.0 MB", "counted by novad");
            compare(Listing.items(1), "1 item");
            compare(Listing.items(5), "5 items");
        }

        // Explorer's "197 GB free of 931 GB": 1024-based, like every size in
        // Nova, one decimal under 10.
        function test_a_drive_reads_what_is_free_of_its_size() {
            const drive = (size, used) => ({
                        id: "d",
                        name: "Games SSD",
                        removable: false,
                        mount: "/mnt/games",
                        size: size,
                        used: used
                    });
            compare(Space.freeOf(drive(1e12, 788e9)), "197 GB free of 931 GB");
            compare(Space.freeOf(drive(64e9, 54.5e9)), "8.8 GB free of 60 GB");
            compare(Space.freeOf(drive(2e12, 2e12)), "0 B free of 1.8 TB");
            compare(Space.freeOf({
                id: "u",
                name: "Stick",
                removable: true,
                size: 32e9
            }), "", "not mounted");
            compare(Space.fill(drive(1e12, 250e9)), 0.25);
        }

        function test_a_drive_almost_full_is_low() {
            const drive = used => ({
                        mount: "/",
                        size: 100,
                        used: used
                    });
            verify(Space.low(drive(91)));
            verify(!Space.low(drive(90)), "a tenth free is enough");
            verify(!Space.low({
                size: 100,
                used: 99
            }), "not mounted, not measured");
        }

        function test_a_folder_is_on_the_innermost_drive_holding_it() {
            const drives = [
                {
                    id: "root",
                    mount: "/"
                },
                {
                    id: "games",
                    mount: "/mnt/games"
                },
                {
                    id: "stick"
                }
            ];
            compare(Space.driveOf(drives, "/mnt/games/steam").id, "games");
            compare(Space.driveOf(drives, "/mnt/gamesaves").id, "root");
            compare(Space.driveOf([drives[2]], "/home"), null);
        }

        function test_a_rows_bar_is_its_share_of_the_biggest() {
            compare(Space.share(50, 200), 0.25);
            compare(Space.share(-1, 200), 0, "a folder not counted yet");
            compare(Space.share(10, -1), 0);
            verify(Space.BIGGEST_FIRST.mixed && Space.BIGGEST_FIRST.descending);
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
            const photos = entry("photos.zip", {
                mime: "application/zip",
                archive: true
            });
            compare(Listing.opening([photos]), {
                folder: "photos.zip",
                files: []
            }, "an archive opens like a folder");
            compare(Listing.icon(photos), Icons.ARCHIVE);
        }

        function test_each_job_shows_what_it_does() {
            compare(Transfers.icon(transfer(1, Protocol.TRANSFER_RUNNING)), Icons.COPY);
            compare(Transfers.icon(transfer(1, Protocol.TRANSFER_RUNNING, {
                mode: Protocol.TRANSFER_EXTRACT_HERE
            })), Icons.EXTRACT);
            compare(Transfers.icon(transfer(1, Protocol.TRANSFER_RUNNING, {
                mode: Protocol.TRANSFER_COMPRESS
            })), Icons.ARCHIVE);
        }

        function transfer(job, state, extra) {
            return Object.assign({
                job: job,
                mode: Protocol.TRANSFER_COPY,
                state: state,
                items: 6,
                name: "cover.png",
                done_bytes: 0,
                total_bytes: 0
            }, extra ?? {});
        }

        readonly property var fileClash: ({
                id: 3,
                name: "cover.png",
                folder: false,
                others: 2,
                original: {
                    size: 1887437,
                    modified_ms: Date.UTC(2026, 9, 12, 9, 50)
                },
                replacement: {
                    size: 2516582
                }
            })

        function test_a_file_url_keeps_raw_bytes_and_escapes_the_rest() {
            const eAcute = String.fromCodePoint(Protocol.RAW_BYTE_BASE + 0xE9);
            compare(Paths.fileUrl(`/mnt/old/caf${eAcute}.png`), "file:///mnt/old/caf%E9.png");
            compare(Paths.fileUrl("/home/ana/#1 café.png"), "file:///home/ana/%231%20caf%C3%A9.png");
        }

        function test_the_preview_pane_previews_one_selected_file() {
            const photo = entry("a.png", {
                mime: "image/png"
            });
            compare(Preview.subject(1, photo), photo);
            compare(Preview.subject(0, null), null);
            compare(Preview.subject(3, photo), null, "several: none of them");
            verify(Preview.asks(photo));
            verify(!Preview.asks(entry("Music", {
                dir: true,
                mime: "inode/directory"
            })), "a folder needs no asking");
            verify(!Preview.asks(null));
        }

        function test_the_preview_pane_face_follows_the_answer() {
            const notes = entry("notes.md", {
                mime: "text/markdown"
            });
            const folder = entry("Music", {
                dir: true,
                mime: "inode/directory"
            });
            const image = {
                kind: Protocol.PREVIEW_IMAGE,
                path: "/home/ana/a.png"
            };
            const text = {
                kind: Protocol.PREVIEW_TEXT,
                text: "# Notes",
                cut: false
            };
            compare(Preview.face(0, null, null, false), Preview.EMPTY);
            compare(Preview.face(2, null, null, false), Preview.COUNT);
            compare(Preview.face(1, folder, null, false), Preview.ITEM, "a folder at once");
            compare(Preview.face(1, notes, null, false), Preview.WAITING);
            compare(Preview.face(1, notes, text, false), Preview.TEXT);
            compare(Preview.face(1, notes, image, false), Preview.IMAGE);
            compare(Preview.face(1, notes, image, true), Preview.ITEM, "a picture Qt can't read");
            compare(Preview.face(1, notes, {
                kind: Protocol.PREVIEW_NONE
            }, false), Preview.ITEM);
            compare(Preview.face(1, notes, false, false), Preview.ITEM, "novad couldn't tell");
        }

        function test_jobs_come_in_order_and_leave_when_they_end() {
            let jobs = Transfers.apply([], transfer(1, Protocol.TRANSFER_COUNTING));
            jobs = Transfers.apply(jobs, transfer(2, Protocol.TRANSFER_RUNNING));
            jobs = Transfers.apply(jobs, transfer(1, Protocol.TRANSFER_RUNNING, {
                done_bytes: 5,
                total_bytes: 10
            }));
            compare(jobs.map(job => job.job), [1, 2], "an update keeps its place");
            compare(Transfers.percent(jobs[0]), "50%");
            compare(Transfers.asking(jobs), null);
            jobs = Transfers.apply(jobs, transfer(2, Protocol.TRANSFER_ASKING, {
                clash: fileClash
            }));
            compare(Transfers.asking(jobs).job, 2);
            jobs = Transfers.apply(jobs, transfer(1, Protocol.TRANSFER_DONE));
            jobs = Transfers.apply(jobs, transfer(2, Protocol.TRANSFER_FAILED, {
                error: "No space left"
            }));
            compare(jobs.map(job => job.state), [Protocol.TRANSFER_FAILED], "a failed job stays to say why");
            compare(Transfers.without(jobs, 2), []);
            verify(Transfers.ended(Protocol.TRANSFER_CANCELLED));
            verify(!Transfers.ended(Protocol.TRANSFER_ASKING));
        }

        function test_a_job_reads_as_its_item_or_how_many() {
            compare(Transfers.label(transfer(1, Protocol.TRANSFER_RUNNING)), "6 items");
            compare(Transfers.label(transfer(1, Protocol.TRANSFER_RUNNING, {
                items: 1
            })), "cover.png");
            compare(Transfers.percent(transfer(1, Protocol.TRANSFER_COUNTING)), "0%");
            const eAcute = String.fromCodePoint(Protocol.RAW_BYTE_BASE + 0xE9);
            compare(Transfers.label(transfer(1, Protocol.TRANSFER_RUNNING, {
                items: 1,
                name: `caf${eAcute}.md`
            })), "caf\uFFFD.md", "a name that isn't UTF-8 shows as Paths.shown has it");
        }

        function test_a_clash_reads_like_cosmic_files() {
            compare(Transfers.clashTitle(fileClash), "“cover.png” already exists in this location");
            compare(Transfers.amount(fileClash.original), "Size: 1.8 MB");
            compare(Transfers.amount({
                items: 214
            }), "Contents: 214 items");
            compare(Transfers.modified(fileClash.replacement), "", "no date when novad has none");
            verify(Transfers.modified(fileClash.original).startsWith("Last modified: Oct 12, 2026"));
            compare(Transfers.allLabel(fileClash), "Apply to all (2)");
            compare([Transfers.defaultAnswer(fileClash), Transfers.defaultLabel(fileClash)], [Protocol.CLASH_REPLACE, "Replace"]);
            const folder = Object.assign({}, fileClash, {
                folder: true
            });
            compare([Transfers.defaultAnswer(folder), Transfers.defaultLabel(folder)], [Protocol.CLASH_MERGE, "Merge"]);
            compare([Transfers.originalLabel(folder), Transfers.replacementLabel(folder)], ["Original folder", "Merge with"]);
        }

        Component {
            id: dialogComponent
            ClashDialog {}
        }

        SignalSpy {
            id: answers
            signalName: "answered"
        }

        SignalSpy {
            id: cancels
            signalName: "cancelled"
        }

        function test_the_dialog_answers_with_its_default_skip_or_for_all() {
            const dialog = createTemporaryObject(dialogComponent, stage, {
                clash: fileClash
            });
            answers.target = dialog;
            cancels.target = dialog;
            answers.clear();
            cancels.clear();
            mouseClick(findText(dialog, "Replace"));
            mouseClick(findText(dialog, "Apply to all (2)"));
            mouseClick(findText(dialog, "Skip"));
            compare(answers.signalArguments.map(args => [args[0], args[1]]), [[Protocol.CLASH_REPLACE, false], [Protocol.CLASH_SKIP, true]]);
            dialog.forceActiveFocus();
            keyClick(Qt.Key_Escape);
            compare(cancels.count, 1);
            dialog.clash = Object.assign({}, fileClash, {
                id: 4,
                others: 0
            });
            verify(!dialog.all, "a new clash starts unticked");
            verify(!findText(dialog, "Apply to all (0)"), "nothing to apply to");
        }

        Component {
            id: cardComponent
            TransferCard {}
        }

        SignalSpy {
            id: cardCancels
            signalName: "cancelled"
        }

        SignalSpy {
            id: dismissals
            signalName: "dismissed"
        }

        function test_the_card_cancels_a_running_job_and_dismisses_a_failed_one() {
            const card = createTemporaryObject(cardComponent, stage, {
                transfer: transfer(1, Protocol.TRANSFER_RUNNING, {
                    done_bytes: 62,
                    total_bytes: 100
                })
            });
            cardCancels.target = card;
            dismissals.target = card;
            cardCancels.clear();
            dismissals.clear();
            verify(findText(card, "6 items"));
            verify(findText(card, "62%"));
            mouseClick(card, card.width - 20, card.height / 2);
            compare(cardCancels.count, 1);
            card.transfer = transfer(1, Protocol.TRANSFER_FAILED, {
                error: "No space left for /run/media/ana/SANDISK/cover.png"
            });
            verify(findText(card, "No space left for /run/media/ana/SANDISK/cover.png"));
            verify(!findText(card, "0%"));
            mouseClick(card, card.width - 20, card.height / 2);
            compare(dismissals.count, 1);
        }

        // A trashed item as novad lists it.
        function trashed(id, name, folder, extra) {
            return {
                id: id,
                folder: folder,
                deleted_ms: Date.UTC(2026, 9, 30, 9, 0),
                entry: Object.assign({
                    name: name,
                    dir: false,
                    link: false,
                    hidden: false,
                    mime: "text/plain",
                    size: 4096,
                    modified_ms: Date.UTC(2026, 9, 31, 9, 12)
                }, extra ?? {})
            };
        }

        function test_the_home_page_is_a_page_with_one_part() {
            verify(Home.isHome(Protocol.FILES_HOME_PLACE));
            verify(Home.isPage(Protocol.FILES_HOME_PLACE) && Home.isPage(Protocol.FILES_TRASH_PLACE));
            verify(!Home.isPage("/home/ana"), "a folder isn't a page");
            compare(names(Home.crumbs()), ["Home"]);
        }

        function test_places_read_by_kind_and_pins_by_their_name() {
            const places = [
                {
                    kind: Protocol.PLACE_HOME,
                    path: Protocol.FILES_HOME_PLACE
                },
                {
                    kind: Protocol.PLACE_DOWNLOADS,
                    path: "/home/ana/Downloads",
                    name: "Downloads"
                },
                {
                    kind: Protocol.PLACE_FOLDER,
                    path: "/home/ana/Projects/Nova",
                    name: "Work"
                },
                {
                    kind: Protocol.PLACE_TRASH,
                    path: Protocol.FILES_TRASH_PLACE
                }
            ];
            compare(places.map(place => Home.look(place).name), ["Home", "Downloads", "Work", "Trash"]);
            compare(places.map(place => Home.look(place).icon), [Icons.HOME, Icons.DOWNLOAD, Icons.FOLDER, Icons.TRASH]);
            compare(Home.pinned(places).map(place => place.path), ["/home/ana/Downloads", "/home/ana/Projects/Nova"]);
            verify(Home.pinnedIn(places, "/home/ana/Projects/Nova"));
            verify(!Home.pinnedIn(places, "/home/ana"), "Home's folder isn't pinned");
            verify(!Home.pinnedIn(places, Protocol.FILES_TRASH_PLACE), "the trash is no pin");
        }

        function test_a_recent_file_is_a_row_with_where_and_when() {
            const row = Home.recentRow({
                folder: "/home/ana/Documents",
                used_ms: Date.UTC(2026, 10, 2, 8, 30),
                entry: {
                    name: "plan.md",
                    dir: false,
                    link: false,
                    hidden: false,
                    mime: "text/markdown",
                    size: 2048,
                    modified_ms: Date.UTC(2026, 10, 1, 9)
                }
            });
            compare([row.name, row.folder, row.used, row.size], ["plan.md", "/home/ana/Documents", Date.UTC(2026, 10, 2, 8, 30), 2048]);
            compare(Home.recentRow({
                folder: "/",
                entry: {
                    name: "x",
                    dir: false,
                    link: false,
                    hidden: false,
                    mime: "text/plain"
                }
            }).used, -1, "no time in the list");
        }

        function test_the_trash_is_a_place_with_one_part() {
            verify(Trash.isTrash(Protocol.FILES_TRASH_PLACE));
            verify(!Trash.isTrash("/home/ana"));
            compare(names(Trash.crumbs()), ["Trash"]);
        }

        function test_a_trashed_row_says_where_it_came_from() {
            const row = Trash.row(trashed("i1", "notes.txt", "/home/ana/Documents"));
            compare([row.trashId, row.name, row.folder, row.size], ["i1", "notes.txt", "/home/ana/Documents", 4096]);
            compare(row.deleted, Date.UTC(2026, 9, 30, 9, 0));
            compare(Trash.row(Object.assign(trashed("i2", "x", "/"), {
                deleted_ms: undefined
            })).deleted, -1, "no date in the trash");
            compare(Paths.located("/home/ana/Documents", "/home/ana"), "~/Documents");
            compare(Paths.located("/home/ana", "/home/ana"), "~");
            compare(Paths.located("/home/anabel", "/home/ana"), "/home/anabel");
            compare(Paths.located("/run/media/ana/SANDISK", ""), "/run/media/ana/SANDISK");
        }

        function test_restore_and_the_footer_follow_the_picked_items() {
            compare(Trash.restoreLabel(0), "Restore all");
            compare(Trash.restoreLabel(2), "Restore");
            compare(Trash.footer(0, 7), "7 items");
            compare(Trash.footer(2, 7), "2 of 7 selected");
            compare(Trash.paths("/home/ana", [
                {
                    name: "a"
                },
                {
                    name: "b c"
                }
            ]), ["/home/ana/a", "/home/ana/b c"]);
            compare(Trash.paths("/", [
                {
                    name: "tmp"
                }
            ]), ["/tmp"]);
        }

        function test_deleting_for_good_asks_in_windows_words() {
            const file = Trash.row(trashed("i1", "invoice.pdf", "/home/ana", {
                mime: "application/pdf",
                size: 245760
            }));
            const one = Trash.confirm([file]);
            compare([one.title, one.question], ["Delete file", "Are you sure you want to permanently delete this file?"]);
            compare(one.item, file);
            const details = Trash.details(file);
            compare(details.slice(0, 2), ["Type: PDF", "Size: 240 KB"]);
            verify(details[2].startsWith("Date modified: Oct 31, 2026"), details[2]);
            const folder = Trash.row(trashed("i2", "old-builds", "/home/ana", {
                dir: true,
                mime: "inode/directory",
                size: undefined
            }));
            compare(Trash.confirm([folder]).title, "Delete folder");
            compare(Trash.details(folder)[0], "Type: Folder");
            verify(!Trash.details(folder).some(line => line.startsWith("Size")), "no size for a folder");
            const many = Trash.confirm([file, folder, file]);
            compare([many.title, many.question, many.item], ["Delete multiple items", "Are you sure you want to permanently delete these 3 items?", null]);
        }

        Component {
            id: trashModelComponent
            TrashModel {}
        }

        function test_long_lists_go_to_novad_in_requests_it_reads() {
            compare(Trash.batches([], 4096), [[]]);
            compare(Trash.batches(["/a", "/b"], 4096), [["/a", "/b"]]);
            const paths = [];
            for (let index = 0; index < 2000; index++)
                paths.push(`/home/ana/Documents/photo-${index}.png`);
            const lists = Trash.batches(paths, Protocol.REQUEST_MAX_BYTES);
            verify(lists.length > 1, "2000 paths don't fit in one request");
            compare([].concat(...lists), paths, "every path goes, in order");
            for (const list of lists)
                verify(JSON.stringify({
                    paths: list
                }).length < Protocol.REQUEST_MAX_BYTES - Trash.REQUEST_ROOM, "each request fits");
        }

        function test_a_copied_row_stays_as_it_was() {
            const rows = createTemporaryQmlObject("import QtQuick; ListModel {}", stage);
            rows.append(Listing.row({
                name: "a.txt",
                dir: false,
                link: false,
                hidden: false,
                mime: "text/plain",
                size: 3,
                modified_ms: 5
            }));
            const copied = Listing.copy(rows.get(0));
            rows.clear();
            compare([copied.name, copied.mime, copied.size, copied.modified], ["a.txt", "text/plain", 3, 5]);
        }

        function test_the_selection_stays_on_items_still_in_the_trash() {
            const model = createTemporaryObject(trashModelComponent, stage);
            model._fill([trashed("a", "a", "/x"), trashed("b", "b", "/x"), trashed("c", "c", "/x")]);
            model.select({
                clear: true,
                from: 1,
                to: 2,
                toggle: false,
                anchor: 1
            });
            compare(model.selectedRows().map(row => row.trashId), ["b", "c"]);
            model._fill([trashed("c", "c", "/x"), trashed("a", "a", "/x")]);
            compare(model.selectedCount, 1, "b left the trash");
            compare(model.selectedRows().map(row => row.trashId), ["c"]);
            compare(model.allRows().length, 2);
            compare(model.anchor, -1, "the anchor's item, b, is gone");
            model.select({
                clear: true,
                from: 1,
                to: 1,
                toggle: false,
                anchor: 1
            });
            const kept = model.selectedRows();
            model._fill([trashed("b", "b", "/x"), trashed("c", "c", "/x"), trashed("a", "a", "/x")]);
            compare(model.anchor, 2, "the anchor follows a as it moves");
            compare(kept[0].trashId, "a", "rows handed out stay as they were");
            model.clearSelection();
            compare(model.selectedCount, 0);
        }

        Component {
            id: confirmComponent
            ConfirmDialog {}
        }

        SignalSpy {
            id: accepts
            signalName: "accepted"
        }

        SignalSpy {
            id: declines
            signalName: "declined"
        }

        function test_the_confirm_dialog_says_yes_or_no() {
            const file = Trash.row(trashed("i1", "invoice.pdf", "/home/ana"));
            const dialog = createTemporaryObject(confirmComponent, stage, {
                asking: Trash.confirm([file])
            });
            accepts.target = dialog;
            declines.target = dialog;
            accepts.clear();
            declines.clear();
            verify(findText(dialog, "Delete file"));
            verify(findText(dialog, "invoice.pdf"));
            verify(findText(dialog, "Type: Text"));
            mouseClick(findText(dialog, "Yes"));
            mouseClick(findText(dialog, "No"));
            dialog.forceActiveFocus();
            keyClick(Qt.Key_Return);
            keyClick(Qt.Key_Escape);
            compare([accepts.count, declines.count], [2, 2]);
            dialog.asking = Trash.confirm([file, file]);
            verify(!findText(dialog, "invoice.pdf"), "several items name none");
        }

        Component {
            id: noticeComponent
            TrashNotice {}
        }

        function test_a_notice_names_the_item_and_dismisses() {
            const notice = createTemporaryObject(noticeComponent, stage, {
                problem: {
                    row: Trash.row(trashed("i1", "notes.txt", "/home/ana/Documents")),
                    error: "A file with this name already exists in Documents."
                }
            });
            dismissals.target = notice;
            dismissals.clear();
            verify(findText(notice, "notes.txt"));
            verify(findText(notice, "A file with this name already exists in Documents."));
            waitForRendering(notice);
            mouseClick(notice, notice.width - 20, notice.height / 2);
            compare(dismissals.count, 1);
        }

        function picker(mode, extra) {
            return Object.assign({
                id: 7,
                mode: mode,
                title: "",
                multiple: false,
                folders: false,
                filters: [],
                filter: 0
            }, extra ?? {});
        }

        function test_a_picker_is_titled_and_labelled_like_windows() {
            compare(Picker.title(picker(Protocol.PICK_OPEN)), "Open");
            compare(Picker.title(picker(Protocol.PICK_OPEN, {
                folders: true
            })), "Select Folder");
            compare(Picker.title(picker(Protocol.PICK_SAVE)), "Save As");
            compare(Picker.title(picker(Protocol.PICK_OPEN, {
                title: "File Upload"
            })), "File Upload");
            compare(Picker.acceptLabel(picker(Protocol.PICK_SAVE_FILES)), "Save");
            compare(Picker.acceptLabel(picker(Protocol.PICK_OPEN, {
                accept: "_Upload"
            })), "Upload", "the app's mnemonic goes");
            compare(Picker.acceptLabel(picker(Protocol.PICK_OPEN, {
                accept: "Add__ons"
            })), "Add_ons");
            compare(Picker.stemLength("cover.tar.png"), 9);
            compare(Picker.stemLength(".bashrc"), 7, "a dot file is all stem");
        }

        function test_accepting_picks_files_or_goes_into_a_folder() {
            const file = name => ({
                        name: name,
                        dir: false
                    });
            const folder = name => ({
                        name: name,
                        dir: true
                    });
            const open = picker(Protocol.PICK_OPEN, {
                multiple: true
            });
            compare(Picker.choice(open, "/home/ana", [file("a.png"), folder("Pics"), file("b.png")], "", null), {
                action: "pick",
                paths: ["/home/ana/a.png", "/home/ana/b.png"]
            });
            compare(Picker.choice(open, "/home/ana", [folder("Pics")], "", null), {
                action: "enter",
                folder: "/home/ana/Pics"
            });
            compare(Picker.choice(open, "/home/ana", [], "", null).action, "none");
            const selectFolder = picker(Protocol.PICK_OPEN, {
                folders: true
            });
            compare(Picker.choice(selectFolder, "/", [folder("srv")], "", null).paths, ["/srv"]);
            compare(Picker.choice(selectFolder, "/home/ana", [], "", null).paths, ["/home/ana"], "nothing selected picks the folder shown");
            compare(Picker.choice(selectFolder, "/srv", [folder("a"), folder("b")], "", null).paths, ["/srv/a", "/srv/b"], "every selected folder");
            const save = picker(Protocol.PICK_SAVE);
            compare(Picker.choice(save, "/home/ana", [], " cover.png ", null).paths, ["/home/ana/cover.png"]);
            compare(Picker.choice(save, "/home/ana", [], "Pics", folder("Pics")).action, "enter");
            for (const name of ["", "..", "a/b"])
                compare(Picker.choice(save, "/home/ana", [], name, null).action, "none", name);
            compare(Picker.filter(picker(Protocol.PICK_OPEN), 0), null);
        }

        Component {
            id: pickerBarComponent
            PickerBar {
                // Room above for the filter menu, which opens upward.
                y: 400
                width: 880
            }
        }

        SignalSpy {
            id: pickerAccepts
            signalName: "accepted"
        }

        function test_the_picker_bar_names_filters_and_answers() {
            const images = {
                name: "Images",
                globs: ["*.png"],
                mimes: []
            };
            const all = {
                name: "All Files",
                globs: ["*"],
                mimes: []
            };
            const bar = createTemporaryObject(pickerBarComponent, stage, {
                picker: picker(Protocol.PICK_SAVE, {
                    name: "cover.png",
                    filters: [all, images],
                    filter: 1
                }),
                filterIndex: 1,
                canAccept: true
            });
            pickerAccepts.target = bar;
            pickerAccepts.clear();
            verify(findText(bar, "Images"));
            verify(findText(bar, "Save"));
            bar.focusName();
            keyClick(Qt.Key_X);
            compare(bar.name, "x.png", "the stem is selected, so typing keeps the type");
            keyClick(Qt.Key_Return);
            compare(pickerAccepts.count, 1);
            mouseClick(findText(bar, "Images"));
            mouseClick(findText(bar, "All Files"));
            compare(bar.filterIndex, 0);
            const open = createTemporaryObject(pickerBarComponent, stage, {
                picker: picker(Protocol.PICK_OPEN),
                canAccept: false
            });
            verify(findText(open, "Open"));
            verify(!open.naming, "opening has no name field");
        }

        function test_ctrl_tab_goes_round_the_tabs() {
            compare(Tabs.cycled(0, 3, 1), 1);
            compare(Tabs.cycled(2, 3, 1), 0, "from the last to the first");
            compare(Tabs.cycled(0, 3, -1), 2, "Ctrl+Shift+Tab from the first to the last");
            compare(Tabs.cycled(0, 1, 1), 0);
        }

        function test_closing_the_shown_tab_shows_the_next_one() {
            compare(Tabs.neighbour(1, 3), 2);
            compare(Tabs.neighbour(2, 3), 1, "the last one's goes back one");
            compare(Tabs.neighbour(0, 2), 1);
        }

        function test_tabs_share_the_row_between_their_widths() {
            compare(Tabs.width(1000, 2, 2, 96, 220), 220, "room to spare: widest");
            compare(Tabs.width(500, 4, 2, 96, 220), 123);
            compare(Tabs.width(300, 8, 2, 96, 220), 96, "crowded: narrowest");
        }

        function test_a_dragged_tab_moves_along_or_comes_out() {
            verify(Tabs.tornOut(60, 40, 2));
            verify(Tabs.tornOut(-60, 40, 2), "above the row too");
            verify(!Tabs.tornOut(20, 40, 2), "near the row it stays");
            verify(!Tabs.tornOut(200, 40, 1), "the only tab stays");
            compare(Tabs.dropped(0, 230, 222, 3), 1);
            compare(Tabs.dropped(2, -1000, 222, 3), 0);
            compare(Tabs.dropped(1, 1000, 222, 3), 2);
            compare(Tabs.dropped(1, 50, 222, 3), 1, "less than half a tab stays");
            compare(Tabs.reordered(["a", "b", "c"], 0, 2), ["b", "c", "a"]);
            compare(Tabs.reordered(["a", "b", "c"], 2, 0), ["c", "a", "b"]);
        }

        // A stand-in for FilesTab, whose Navigator needs novad.
        function test_the_shown_tab_scrolls_into_view() {
            compare(Tabs.revealed(0, 100, 96, 400), 0, "in view: stays");
            compare(Tabs.revealed(0, 500, 96, 400), 196, "past the end: its end at the view's");
            compare(Tabs.revealed(300, 98, 96, 400), 98, "before the start: its start at the view's");
        }

        Component {
            id: tabComponent
            QtObject {
                property string title
                property bool trash
            }
        }
        Component {
            id: tabRowComponent
            TabRow {
                width: 880
                raised: "#202020"
            }
        }
        SignalSpy {
            id: tabsPicked
            signalName: "picked"
        }
        SignalSpy {
            id: tabsClosed
            signalName: "closeRequested"
        }

        function test_the_tab_row_shows_closes_and_names_its_tabs() {
            const music = createTemporaryObject(tabComponent, stage, {
                title: "Music"
            });
            const trash = createTemporaryObject(tabComponent, stage, {
                title: Trash.name(),
                trash: true
            });
            const row = createTemporaryObject(tabRowComponent, stage, {
                tabs: [music, trash],
                current: music
            });
            tabsPicked.target = row;
            tabsClosed.target = row;
            tabsPicked.clear();
            tabsClosed.clear();
            waitForRendering(row);
            compare(row.tabWidth, Tokens.filesTabMaxWidth);
            verify(findText(row, "Music"));
            const trashTitle = findText(row, Trash.name());
            verify(trashTitle);
            mouseClick(trashTitle);
            compare(tabsPicked.count, 1);
            compare(tabsPicked.signalArguments[0][0], trash);
            // The shown tab's × is at its right end.
            const musicTitle = findText(row, "Music");
            mouseClick(musicTitle.parent, musicTitle.parent.width - Tokens.filesTabHeight / 2, musicTitle.parent.height / 2);
            compare(tabsClosed.count, 1);
            compare(tabsClosed.signalArguments[0][0], music);
            compare(tabsPicked.count, 1, "the × doesn't also show it");
            mouseClick(trashTitle, 1, 1, Qt.MiddleButton);
            compare(tabsClosed.count, 2, "a middle click closes");
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
        // files_search's answer as novad sends it (the wire format test's
        // recorded line), for a game in Steam's hidden folder.
        function foundGame() {
            return {
                entry: {
                    dir: true,
                    hidden: false,
                    link: false,
                    mime: "inode/directory",
                    modified_ms: 1793000000000,
                    name: "Crimson Desert"
                },
                folder: "/home/user/.local/share/Steam/steamapps/common",
                in_hidden_folder: true,
                marks: [
                    {
                        length: 7,
                        start: 0
                    }
                ]
            };
        }

        function test_a_result_reads_with_its_match_and_where_it_lives() {
            const row = Search.row(foundGame());
            compare([row.name, row.dir, row.size, row.dim], ["Crimson Desert", true, -1, true], "inside a hidden folder reads dimmer");
            compare(Search.path(row), "/home/user/.local/share/Steam/steamapps/common/Crimson Desert");
            compare(Search.location(row.folder, "/home/user"), "~/.local/share/Steam/steamapps/common");
            compare(Search.location("/run/media/user/SANDISK", "/home/user"), "/run/media/user/SANDISK");
            compare(Search.location("/home/username", "/home/user"), "/home/username", "only Home itself, not a name it starts");
            compare(Search.location("/home/user/Pictures", "/home/user/"), "~/Pictures", "Home with a slash at its end");
            compare(Search.marked(row.name, row.marks, "#fd7014"), '<b><font color="#fd7014">Crimson</font></b> Desert');
        }

        function test_a_marked_name_stays_plain_text() {
            compare(Search.marked("<b>a&b</b>", [], "#fff"), "&lt;b&gt;a&amp;b&lt;/b&gt;");
            compare(Search.marked("notes notes", [
                {
                    start: 6,
                    length: 5
                },
                {
                    start: 0,
                    length: 5
                }
            ], "#fff"), '<b><font color="#fff">notes</font></b> <b><font color="#fff">notes</font></b>', "marks in any order");
            const raw = String.fromCodePoint(Protocol.RAW_BYTE_BASE + 0xE9);
            compare(Search.marked("caf" + raw, [
                {
                    start: 0,
                    length: 3
                }
            ], "#fff"), '<b><font color="#fff">caf</font></b>\uFFFD', "a raw byte shows as a replacement character");
        }

        function test_the_count_says_how_many_were_found() {
            compare(Search.count(false, 0, 0), "Indexing");
            compare(Search.count(true, 0, 0), "No results");
            compare(Search.count(true, 1, 1), "1 result");
            compare(Search.count(true, 4, 4), "4 results");
            compare(Search.count(true, 1532, 200), "200 of 1532 results");
        }

        Component {
            id: searchModelComponent
            SearchModel {
                settle: 0
            }
        }

        // Stands in for FilesService: keeps each search asked, for the
        // test to answer.
        Component {
            id: askedSearchesComponent
            QtObject {
                property var asked: []
                property bool connected: true
                signal searchReady
                function search(query: string, within: string, done: var) {
                    asked = asked.concat([
                        {
                            query: query,
                            within: within,
                            done: done
                        }
                    ]);
                }
            }
        }

        function askedSearches() {
            return createTemporaryObject(askedSearchesComponent, stage);
        }

        function test_typing_asks_novad_and_only_the_latest_answer_counts() {
            const service = askedSearches();
            const model = createTemporaryObject(searchModelComponent, stage, {
                service: service,
                within: "/home/user"
            });
            model.query = "crim";
            tryVerify(() => service.asked.length === 1);
            model.query = " crimson ";
            tryVerify(() => service.asked.length === 2);
            compare([service.asked[1].query, service.asked[1].within], ["crimson", "/home/user"], "spaces around it aside");
            service.asked[1].done({
                ready: true,
                total: 1,
                results: [foundGame()]
            });
            service.asked[0].done({
                ready: true,
                total: 0,
                results: []
            });
            compare(model.rows.map(row => row.name), ["Crimson Desert"], "the older answer came late and is dropped");
            verify(!model.everywhere);
            verify(model.answered);
            model.within = "";
            tryVerify(() => service.asked.length === 3);
            compare(service.asked[2].within, "", "everywhere");
            model.query = "  ";
            compare([model.active, model.rows.length, model.total], [false, 0, 0], "nothing typed, nothing found");
            compare(service.asked.length, 3, "and nothing asked");
        }

        function test_once_the_index_is_built_it_asks_again() {
            const service = askedSearches();
            const model = createTemporaryObject(searchModelComponent, stage, {
                service: service
            });
            model.query = "crimson";
            tryVerify(() => service.asked.length === 1);
            service.asked[0].done({
                ready: false,
                total: 0,
                results: []
            });
            verify(!model.ready);
            wait(10);
            compare(service.asked.length, 1, "nothing is asked until novad says");
            service.searchReady();
            compare(service.asked.length, 2);
            service.asked[1].done({
                ready: true,
                total: 1,
                results: [foundGame()]
            });
            verify(model.ready);
            compare(model.rows.length, 1);
            service.searchReady();
            compare(service.asked.length, 2, "a ready index isn't asked again");
            model.query = "desert";
            tryVerify(() => service.asked.length === 3);
            service.asked[2].done({
                ready: false,
                total: 0,
                results: []
            });
            compare(service.asked.length, 4, "an answer from before novad said it's built is asked again");
        }

        function test_a_search_novad_could_not_answer_is_asked_on_reconnecting() {
            const service = askedSearches();
            const model = createTemporaryObject(searchModelComponent, stage, {
                service: service
            });
            model.query = "desert";
            tryVerify(() => service.asked.length === 1);
            service.asked[0].done(null);
            verify(!model.answered, "no count");
            service.connected = false;
            service.connected = true;
            compare(service.asked.length, 2);
        }

        function test_an_answer_coming_while_typing_settles_is_dropped() {
            const service = askedSearches();
            const model = createTemporaryObject(searchModelComponent, stage, {
                service: service,
                settle: 1000
            });
            model.query = "crim";
            tryVerify(() => service.asked.length === 1, 2000);
            model.query = "crimson";
            service.asked[0].done({
                ready: true,
                total: 1,
                results: [foundGame()]
            });
            compare([model.answered, model.rows.length], [false, 0], "it was for what was typed before");
        }
    }
}
