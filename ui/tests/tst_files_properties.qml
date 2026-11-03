pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../files"
import "../files/properties.js" as Props
import "../theme/icons.js" as Icons

// Nova Files' Properties dialog: what it's shown for (a selection, the
// folder, a drive), when the size is counted, how sizes, counts and the
// measure's jobs add up, the rows it reads for a file, a folder, a
// selection and a drive, the access grid, and the dialog itself: its
// rows, ticking access only when the item is yours, and choosing the app
// that opens it.
Item {
    id: stage
    width: 880
    height: 700

    // novad's files_properties answer for a text file.
    readonly property var notes: ({
            folder: "/home/ana/Documents",
            entry: {
                name: "notes.txt",
                dir: false,
                link: false,
                hidden: false,
                mime: "text/plain",
                size: 1400000,
                modified_ms: Date.UTC(2026, 10, 2, 9, 30)
            },
            disk_bytes: 1404928,
            created_ms: Date.UTC(2026, 9, 1, 8, 0),
            accessed_ms: Date.UTC(2026, 10, 3, 0, 15),
            owner: "ana",
            group: "users",
            access: {
                owner: {
                    read: true,
                    write: true,
                    run: false
                },
                group: {
                    read: true,
                    write: false,
                    run: false
                },
                others: {
                    read: false,
                    write: false,
                    run: false
                }
            },
            yours: true
        })

    function row(name, dir, mime) {
        return {
            name: name,
            dir: dir,
            link: false,
            hidden: false,
            mime: mime,
            size: dir ? -1 : 10,
            modified: -1
        };
    }

    function labels(groups) {
        return groups.map(group => group.map(pair => pair.label));
    }

    function factValue(groups, label) {
        for (const group of groups)
            for (const pair of group)
                if (pair.label === label)
                    return pair.value;
        return undefined;
    }

    Component {
        id: dialogComponent
        PropertiesDialog {}
    }

    TestCase {
        name: "FilesProperties"
        when: windowShown

        function test_what_properties_are_shown_for() {
            const one = Props.subject("/home/ana", [stage.row("notes.txt", false, "text/plain")], null);
            compare(one.paths, ["/home/ana/notes.txt"]);
            compare(one.title, "notes.txt");
            const none = Props.subject("/home/ana/Music", [], null);
            compare(none.paths, ["/home/ana/Music"], "nothing selected is the folder itself");
            compare(none.title, "Music");
            const two = Props.subject("/", [stage.row("a", true, "inode/directory"), stage.row("b", false, "text/plain")], null);
            compare(two.paths, ["/a", "/b"]);
            compare(two.title, "2 items");
            const drive = Props.subject("/home/ana", [], {
                name: "SANDISK",
                mount: "/run/media/ana/SANDISK"
            });
            compare(drive.paths, ["/run/media/ana/SANDISK"]);
            verify(drive.drive);
        }

        function test_only_folders_and_selections_are_counted() {
            verify(!Props.measures(Props.subject("/home/ana", [stage.row("notes.txt", false, "text/plain")], null)), "one file's size is its own");
            verify(Props.measures(Props.subject("/home/ana", [stage.row("Music", true, "inode/directory")], null)));
            verify(Props.measures(Props.subject("/home/ana", [], null)));
            verify(!Props.measures(Props.subject("/", [], {
                name: "Disk",
                mount: "/"
            })), "a drive's fill says how much it holds");
        }

        function test_sizes_and_counts_read_as_in_explorer() {
            compare(Props.size(12), "12 bytes");
            compare(Props.size(1), "1 byte");
            compare(Props.size(1400000), "1.3 MB (1,400,000 bytes)");
            compare(Props.contains(120, 14), "120 files, 14 folders");
            compare(Props.contains(1, 0), "1 file");
            compare(Props.contains(0, 1), "1 folder");
            compare(Props.contains(0, 0), "Empty");
            compare(Props.grouped(1234567), "1,234,567");
        }

        function test_a_measure_adds_up_its_jobs_and_is_done_once_all_are() {
            const counted = (job, files, bytes, done) => ({
                        job: job,
                        files: files,
                        folders: 1,
                        bytes: bytes,
                        disk_bytes: bytes,
                        done: done
                    });
            const waiting = Props.total({}, [1, 2]);
            compare(waiting.bytes, 0);
            verify(!waiting.done);
            const half = Props.total({
                "1": counted(1, 3, 100, true)
            }, [1, 2]);
            compare([half.files, half.bytes], [3, 100]);
            verify(!half.done, "job 2 hasn't reported");
            const all = Props.total({
                "1": counted(1, 3, 100, true),
                "2": counted(2, 2, 50, true)
            }, [1, 2]);
            compare([all.files, all.folders, all.bytes], [5, 2, 150]);
            verify(all.done);
        }

        function test_a_file_reads_its_facts_sizes_and_times() {
            const subject = Props.subject("/home/ana/Documents", [stage.row("notes.txt", false, "text/plain")], null);
            const groups = Props.groups(subject, stage.notes, null, {
                name: "Text Editor"
            });
            compare(labels(groups), [["Type", "Opens with", "Location"], ["Size", "Size on disk"], ["Created", "Modified", "Accessed"]]);
            compare(factValue(groups, "Type"), "Text");
            compare(factValue(groups, "Opens with"), "Text Editor");
            compare(factValue(groups, "Location"), "/home/ana/Documents");
            compare(factValue(groups, "Size"), "1.3 MB (1,400,000 bytes)");
            verify(groups[0][1].change, "the app can be changed");
            const unknown = Props.groups(subject, stage.notes, null, null);
            compare(labels(unknown)[0], ["Type", "Location"], "no app, no row");
        }

        function test_a_folder_counts_what_it_contains() {
            const subject = Props.subject("/home/ana", [], null);
            const folder = Object.assign({}, stage.notes, {
                folder: "/home",
                entry: Object.assign({}, stage.notes.entry, {
                    name: "ana",
                    dir: true,
                    mime: "inode/directory",
                    size: undefined
                }),
                disk_bytes: undefined
            });
            const groups = Props.groups(subject, folder, {
                files: 120,
                folders: 14,
                bytes: 2048,
                disk_bytes: 4096,
                done: false
            }, null);
            compare(labels(groups)[1], ["Size", "Size on disk", "Contains"]);
            compare(factValue(groups, "Contains"), "120 files, 14 folders");
            verify(groups[1].every(row => row.counting), "still counting");
            compare(factValue(groups, "Type"), "Folder");
        }

        function test_a_selection_reads_its_totals() {
            const rows = [stage.row("a.png", false, "image/png"), stage.row("b.png", false, "image/png"), stage.row("Old", true, "inode/directory")];
            const subject = Props.subject("/home/ana/Pictures", rows, null);
            const groups = Props.groups(subject, null, {
                files: 4,
                folders: 1,
                bytes: 3000,
                disk_bytes: 8192,
                done: true
            }, null);
            compare(factValue(groups, "Type"), "Multiple types");
            compare(factValue(groups, "Location"), "All in /home/ana/Pictures");
            compare(factValue(groups, "Contains"), "6 files, 2 folders", "the selected items count too");
            verify(!groups[1].some(row => row.counting), "counted");
            const pngs = Props.subject("/home/ana/Pictures", rows.slice(0, 2), null);
            compare(factValue(Props.groups(pngs, null, null, null), "Type"), "All PNG");
            compare(Props.icon(subject, null), Icons.COPY);
        }

        function test_a_drive_reads_how_full_it_is() {
            const subject = Props.subject("/home/ana", [], {
                name: "SANDISK",
                mount: "/run/media/ana/SANDISK"
            });
            const drive = Object.assign({}, stage.notes, {
                drive: {
                    size: 64000000000,
                    used: 16000000000
                }
            });
            const groups = Props.groups(subject, drive, null, null);
            compare(labels(groups), [["Type", "Location"], ["Used", "Free", "Capacity"]]);
            compare(factValue(groups, "Free"), Props.size(48000000000));
            compare(Props.fill(drive.drive), 0.25);
            compare(Props.icon(subject, drive), Icons.DISK);
        }

        function test_access_reads_per_person_and_flips_one_right() {
            const people = Props.people(stage.notes);
            compare(people.map(person => person.name), ["ana", "users", ""]);
            compare(people[0].rights, stage.notes.access.owner);
            compare(Props.rightsColumns(false).map(column => column.label), ["Read", "Change", "Run"]);
            compare(Props.rightsColumns(true)[2].label, "Open", "a folder's run is going in");
            const flipped = Props.flipped(stage.notes.access, "group", "write");
            verify(flipped.group.write);
            verify(!stage.notes.access.group.write, "the original stays");
            compare(flipped.owner, stage.notes.access.owner);
        }

        function test_the_dialog_ticks_access_only_when_its_yours() {
            const dialog = createTemporaryObject(dialogComponent, stage, {
                subject: Props.subject("/home/ana/Documents", [stage.row("notes.txt", false, "text/plain")], null),
                properties: stage.notes
            });
            const flips = signalSpy.createObject(stage, {
                target: dialog,
                signalName: "accessFlipped"
            });
            waitForRendering(dialog);
            verify(dialog.height > 0);
            // The group's Change box: the access grid's second row, middle column.
            const checks = findChecks(dialog);
            compare(checks.length, 9, "three people, three rights");
            compare(checks.map(check => check.checked), [true, true, false, true, false, false, false, false, false]);
            mouseClick(checks[4]);
            compare(flips.count, 1);
            compare(Array.from(flips.signalArguments[0]), ["group", "write"]);
            dialog.properties = Object.assign({}, stage.notes, {
                yours: false
            });
            waitForRendering(dialog);
            mouseClick(findChecks(dialog)[4]);
            compare(flips.count, 1, "someone else's file can't be changed");
        }

        function test_the_dialog_offers_the_other_apps() {
            const dialog = createTemporaryObject(dialogComponent, stage, {
                subject: Props.subject("/home/ana/Documents", [stage.row("notes.txt", false, "text/plain")], null),
                properties: stage.notes,
                apps: [
                    {
                        id: "editor.desktop",
                        name: "Text Editor"
                    },
                    {
                        id: "vim.desktop",
                        name: "Vim"
                    }
                ],
                defaultApp: "editor.desktop"
            });
            const chosen = signalSpy.createObject(stage, {
                target: dialog,
                signalName: "defaultChosen"
            });
            waitForRendering(dialog);
            dialog.choosingApp = true;
            waitForRendering(dialog);
            const vim = findText(dialog, "Vim");
            verify(vim !== null);
            mouseClick(vim);
            compare(Array.from(chosen.signalArguments[0]), ["vim.desktop"]);
            verify(!dialog.choosingApp);
        }

        function findChecks(item) {
            let found = [];
            for (const child of item.children) {
                if (child instanceof CheckMark)
                    found.push(child);
                else
                    found = found.concat(findChecks(child));
            }
            return found;
        }

        function findText(item, text) {
            for (const child of item.children) {
                if (child.text === text && child.visible)
                    return child;
                const inner = findText(child, text);
                if (inner)
                    return inner;
            }
            return null;
        }
    }

    Component {
        id: signalSpy
        SignalSpy {}
    }
}
