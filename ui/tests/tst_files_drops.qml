pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../files"
import "../theme"
import "../files/drops.js" as Drops
import "../daemon/protocol.js" as Protocol

// Nova Files' drag and drop: what's dragged as text/uri-list and read back
// from other apps' drags, and a DropTarget following a drag over a list,
// the tree and the trash, showing and doing what novad answers (its rules
// are tested in novad: transfers.rs), Ctrl changing mid-drag, a folder
// opened by resting on it, and dropping. The root is an Item: a TestCase
// root gets no mouse events.
Item {
    id: stage
    width: 400
    height: 400

    TestCase {
        name: "FilesDrops"
        when: windowShown

        readonly property string raw: String.fromCodePoint(Protocol.RAW_BYTE_BASE + 0xE9)

        function test_paths_go_out_as_file_uris_and_come_back() {
            const paths = ["/home/ana/My Photos", "/home/ana/café #1.txt", "/home/ana/100%", `/home/ana/caf${raw}`];
            const list = Drops.uriList(paths);
            compare(list, "file:///home/ana/My%20Photos\r\nfile:///home/ana/caf%C3%A9%20%231.txt\r\nfile:///home/ana/100%25\r\nfile:///home/ana/caf%E9\r\n", "a raw byte goes as itself");
            const urls = list.split("\r\n").filter(line => line !== "");
            compare(Drops.localPaths(urls), paths);
        }

        function test_other_apps_uris_are_read_as_they_come() {
            compare(Drops.localPaths(["file://localhost/tmp/a%20b", "file:///tmp/café"]), ["/tmp/a b", "/tmp/café"], "a host of localhost, a name not encoded");
            compare(Drops.localPaths([Qt.resolvedUrl("file:///tmp/x y")]), ["/tmp/x y"], "a url value too");
            compare(Drops.localPaths(["file:///tmp/a", "https://example.com/cat.png"]), [], "a link isn't a file here");
            compare(Drops.localPaths(["file://otherhost/tmp/a"]), [], "nor is a file on another machine");
        }

        function test_the_trash_is_a_place_of_its_own() {
            verify(Drops.place(Protocol.FILES_TRASH_PLACE, false).trash);
            verify(!Drops.place("/home/ana", false).trash);
        }

        function test_the_pointer_shows_copy_move_or_nothing() {
            compare(Drops.dragAction(Protocol.TRANSFER_COPY), Qt.CopyAction);
            compare(Drops.dragAction(Protocol.TRANSFER_MOVE), Qt.MoveAction);
            compare(Drops.dragAction(Drops.TRASH), Qt.MoveAction);
            compare(Drops.dragAction(Drops.NONE), Qt.IgnoreAction);
        }

        Component {
            id: targetComponent
            DropTarget {
                anchors.fill: parent
                service: novad
            }
        }

        // Stands in for FilesService: answers files_drop from `modes`
        // ("folder|copy" to a mode, or null where nothing drops), at once,
        // or once `release()` is called while `holding`.
        QtObject {
            id: novad

            property var modes: ({})
            property bool holding: false
            property var held: []
            property int asked: 0

            function drop(sources, folder, copy, done) {
                asked++;
                const answer = () => done(modes[folder + "|" + copy] ?? null);
                if (holding)
                    held.push(answer);
                else
                    answer();
            }

            function release() {
                const answers = held;
                held = [];
                holding = false;
                for (const answer of answers)
                    answer();
            }
        }

        SignalSpy {
            id: drops
            signalName: "dropRequested"
        }

        SignalSpy {
            id: springs
            signalName: "springOpened"
        }

        // A drag as a DropArea hands it over: where it is, what it carries,
        // and what the target says back.
        function dragOf(paths, x, y, proposed) {
            const drag = {
                x: x,
                y: y,
                urls: Drops.uriList(paths).split("\r\n").filter(line => line !== ""),
                proposedAction: proposed ?? Qt.MoveAction,
                accepted: false,
                action: Qt.IgnoreAction,
                acceptedWith: null
            };
            drag.accept = action => {
                drag.accepted = true;
                drag.acceptedWith = action;
            };
            return drag;
        }

        // What the list shows: /home/ana with the folder Photos in its top
        // half; once Photos opens, its folder 2025 there instead.
        property string shown: "/home/ana"

        function listAt(x, y) {
            if (shown === "/home/ana/Photos")
                return y < 200 ? Drops.place("/home/ana/Photos/2025", true) : Drops.place(shown, false);
            return y < 200 ? Drops.place("/home/ana/Photos", true) : Drops.place(shown, false);
        }

        function target(folderAt, modes) {
            novad.modes = modes;
            novad.holding = false;
            novad.held = [];
            novad.asked = 0;
            shown = "/home/ana";
            const made = createTemporaryObject(targetComponent, stage, {
                folderAt: folderAt
            });
            drops.target = made;
            springs.target = made;
            drops.clear();
            springs.clear();
            return made;
        }

        function dropped(index) {
            return Array.from(drops.signalArguments[index]);
        }

        function test_dragging_a_folder_within_the_list() {
            const list = target(listAt, {
                "/home/ana/Photos|false": Protocol.TRANSFER_MOVE,
                "/home/ana|false": null
            });
            const drag = dragOf(["/home/ana/Backup"], 10, 50);
            list.enter(drag);
            verify(drag.accepted);
            compare(list.hovered.folder, "/home/ana/Photos");
            compare(list.action, Protocol.TRANSFER_MOVE);
            compare(drag.action, Qt.MoveAction);

            drag.y = 300;
            list.follow(drag);
            compare(list.hovered.folder, "/home/ana");
            verify(!drag.accepted, "Backup is already in Home");

            drag.y = 50;
            list.follow(drag);
            verify(drag.accepted);
            compare(novad.asked, 2, "each answer is asked once");
            list.release(dragOf([], 10, 50));
            compare(drops.count, 1);
            compare(dropped(0), [Protocol.TRANSFER_MOVE, ["/home/ana/Backup"], "/home/ana/Photos"]);
            compare(list.hovered, null, "the drag is over");
        }

        function test_ctrl_pressed_or_let_go_mid_drag_counts() {
            const list = target(listAt, {
                "/home/ana/Photos|false": Protocol.TRANSFER_MOVE,
                "/home/ana/Photos|true": Protocol.TRANSFER_COPY
            });
            const drag = dragOf(["/home/ana/Backup"], 10, 50);
            list.enter(drag);
            compare(list.action, Protocol.TRANSFER_MOVE);
            drag.proposedAction = Qt.CopyAction;
            list.follow(drag);
            compare(list.action, Protocol.TRANSFER_COPY);
            compare(drag.action, Qt.CopyAction);
            const letGo = dragOf([], 10, 50, Qt.MoveAction);
            list.release(letGo);
            compare(dropped(0)[0], Protocol.TRANSFER_MOVE, "Ctrl let go before the drop moves");
            compare(letGo.acceptedWith, Qt.MoveAction);
        }

        function test_resting_on_a_folder_opens_it_and_the_drop_goes_where_it_lands() {
            const list = target(listAt, {
                "/home/ana/Photos|false": Protocol.TRANSFER_MOVE,
                "/home/ana/Photos/2025|false": Protocol.TRANSFER_MOVE
            });
            springs.target.springOpened.connect(folder => shown = folder);
            list.enter(dragOf(["/home/ana/Backup"], 10, 50));
            springs.wait(Tokens.durationFilesSpring * 3);
            compare(springs.signalArguments[0][0], "/home/ana/Photos");
            compare(list.hovered, null, "what's under the pointer changed");
            list.release(dragOf([], 10, 50));
            compare(dropped(0), [Protocol.TRANSFER_MOVE, ["/home/ana/Backup"], "/home/ana/Photos/2025"], "into the folder now under the pointer");
        }

        function test_dragging_a_folder_into_itself_is_refused() {
            const list = target(listAt, {
                "/home/ana/Photos|false": null
            });
            const drag = dragOf(["/home/ana/Photos"], 10, 50);
            list.enter(drag);
            verify(drag.accepted, "the list still follows the drag");
            compare(drag.action, Qt.IgnoreAction, "but the pointer says no");
            wait(Tokens.durationFilesSpring * 2);
            compare(springs.count, 0, "a refused folder doesn't open");
            const letGo = dragOf([], 10, 50);
            list.release(letGo);
            verify(!letGo.accepted);
            compare(drops.count, 0, "nothing starts");
        }

        function test_a_drop_before_novad_answers_waits_for_it() {
            const list = target(listAt, {
                "/home/ana/Photos|false": Protocol.TRANSFER_COPY
            });
            novad.holding = true;
            const drag = dragOf(["/run/media/ana/STICK/a.png"], 10, 50);
            list.enter(drag);
            compare(list.action, Drops.NONE, "nothing until novad says");
            const letGo = dragOf([], 10, 50);
            list.release(letGo);
            verify(letGo.accepted);
            compare(drops.count, 0);
            novad.release();
            compare(dropped(0), [Protocol.TRANSFER_COPY, ["/run/media/ana/STICK/a.png"], "/home/ana/Photos"], "another drive copies");
        }

        function test_dragging_a_folder_onto_the_tree() {
            const rows = [Drops.place("/home/ana", false), Drops.place("/home/ana/Documents", true)];
            const tree = target((x, y) => rows[Math.floor(y / 200)] ?? Drops.NOWHERE, {
                "/home/ana/Documents|true": Protocol.TRANSFER_COPY
            });
            const drag = dragOf(["/home/ana/Photos/2025"], 10, 250, Qt.CopyAction);
            tree.enter(drag);
            compare(tree.action, Protocol.TRANSFER_COPY, "Ctrl copies");
            compare(drag.action, Qt.CopyAction);
            tree.release(dragOf([], 10, 250, Qt.CopyAction));
            compare(dropped(0), [Protocol.TRANSFER_COPY, ["/home/ana/Photos/2025"], "/home/ana/Documents"]);
        }

        function test_dragging_a_folder_onto_the_trash() {
            const trash = target(() => Drops.place(Protocol.FILES_TRASH_PLACE, false), {});
            const drag = dragOf(["/home/ana/Photos", "/home/ana/a.txt"], 10, 10);
            trash.enter(drag);
            compare(drag.action, Qt.MoveAction);
            compare(novad.asked, 0, "the trash takes anything");
            trash.release(dragOf([], 10, 10));
            compare(dropped(0), [Drops.TRASH, ["/home/ana/Photos", "/home/ana/a.txt"], Protocol.FILES_TRASH_PLACE]);
            compare(springs.count, 0, "the trash doesn't open");
        }

        function test_a_browser_link_drops_nowhere() {
            const list = target(listAt, {});
            const drag = dragOf([], 10, 50);
            drag.urls = ["https://example.com/cat.png"];
            list.enter(drag);
            verify(!drag.accepted);
        }
    }
}
