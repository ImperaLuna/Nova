pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../files"
import "../files/paths.js" as Paths
import "../files/navigation.js" as Navigation
import "../files/transfers.js" as Transfers
import "../files/trash.js" as Trash
import "../daemon/protocol.js" as Protocol

// Nova Files' address bar parts and window title from a folder's path,
// its history, the sidebar tree's kept folders and rows, and the copy and
// move jobs a window keeps and how their card and name clash dialog read
// and answer, and the trash: its rows, its selection, and the dialog that
// asks before deleting for good. The root is an Item: a TestCase root gets no mouse events.
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
            compare(Trash.location("/home/ana/Documents", "/home/ana"), "~/Documents");
            compare(Trash.location("/home/ana", "/home/ana"), "~");
            compare(Trash.location("/home/anabel", "/home/ana"), "/home/anabel");
            compare(Trash.location("/run/media/ana/SANDISK", ""), "/run/media/ana/SANDISK");
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
