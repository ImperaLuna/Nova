pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../files"
import "../theme"
import "../files/drops.js" as Drops
import "../daemon/protocol.js" as Protocol

// Nova Files' drag and drop: what's dragged as text/uri-list and read back
// from other apps' drags, what a drop on a folder does (Explorer's rules:
// same drive moves, another copies, Ctrl copies, the trash trashes, a
// folder never goes into itself), and a DropTarget following a drag over
// a list, the tree and the trash, and dropping. The root is an Item: a
// TestCase root gets no mouse events.
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

        // Dragging a folder within the list: onto a folder beside it.
        function test_a_folder_dropped_on_a_folder_beside_it_moves_on_one_drive() {
            const photos = ["/home/ana/Photos"];
            compare(Drops.decide(photos, "/home/ana/Backup", false, false, true), Protocol.TRANSFER_MOVE);
            compare(Drops.decide(photos, "/home/ana/Backup", false, false, null), Protocol.TRANSFER_MOVE, "a move until novad says otherwise");
            compare(Drops.decide(photos, "/run/media/ana/STICK", false, false, false), Protocol.TRANSFER_COPY, "another drive copies");
            compare(Drops.decide(photos, "/home/ana/Backup", false, true, true), Protocol.TRANSFER_COPY, "Ctrl copies");
            compare(Drops.decide(["/home/ana/Photos", "/home/ana/notes.txt"], "/home/ana/Backup", false, false, true), Protocol.TRANSFER_MOVE, "files and folders together");
        }

        function test_a_drop_where_the_items_already_are_moves_nothing() {
            compare(Drops.decide(["/home/ana/Photos"], "/home/ana", false, false, true), Drops.NONE);
            compare(Drops.decide(["/home/ana/Photos"], "/home/ana", false, true, true), Protocol.TRANSFER_COPY, "Ctrl makes a numbered copy");
            compare(Drops.decide(["/home/ana/Photos", "/tmp/a"], "/home/ana", false, false, true), Protocol.TRANSFER_MOVE, "one from elsewhere comes");
        }

        function test_a_folder_never_goes_into_itself() {
            const photos = ["/home/ana/Photos"];
            compare(Drops.decide(photos, "/home/ana/Photos", false, false, true), Drops.NONE);
            compare(Drops.decide(photos, "/home/ana/Photos/2025/May", false, false, true), Drops.NONE, "nor a folder of its own");
            compare(Drops.decide(photos, "/home/ana/Photos/2025", false, true, true), Drops.NONE, "not even as a copy");
            compare(Drops.decide(photos, "/home/ana/Photos 2", false, false, true), Protocol.TRANSFER_MOVE, "a name it starts is elsewhere");
            compare(Drops.decide([], "/home/ana", false, false, true), Drops.NONE);
        }

        function test_the_trash_takes_anything() {
            compare(Drops.decide(["/home/ana/Photos", "/home/ana/a.txt"], Protocol.FILES_TRASH_PLACE, true, false, null), Drops.TRASH);
            compare(Drops.decide(["/home/ana/Photos"], Protocol.FILES_TRASH_PLACE, true, true, null), Drops.TRASH, "Ctrl too");
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
                service: drives
            }
        }

        // Stands in for FilesService: says nothing, as novad does while it's
        // away, so a test sets the drive answers itself.
        QtObject {
            id: drives
            function sameDrive(sources, folder, done) {
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

        // A list showing /home/ana with the folder Photos in its top half,
        // as FileList's folderAt sees it.
        function listAt(x, y) {
            return y < 200 ? Drops.place("/home/ana/Photos", true) : Drops.place("/home/ana", false);
        }

        function target(folderAt) {
            const made = createTemporaryObject(targetComponent, stage, {
                folderAt: folderAt
            });
            drops.target = made;
            springs.target = made;
            drops.clear();
            springs.clear();
            return made;
        }

        function test_dragging_a_folder_within_the_list() {
            const list = target(listAt);
            const drag = dragOf(["/home/ana/Backup"], 10, 50);
            list.sameDrives = {};
            list.enter(drag);
            verify(drag.accepted);
            compare(list.hovered.folder, "/home/ana/Photos");
            list.sameDrives = {
                "/home/ana/Photos": true,
                "/home/ana": true
            };
            compare(list.action, Protocol.TRANSFER_MOVE);
            list.follow(drag);
            compare(drag.action, Qt.MoveAction);
            springs.wait(Tokens.durationFilesSpring * 3);
            compare(springs.signalArguments[0][0], "/home/ana/Photos", "resting on a folder opens it");

            drag.y = 300;
            list.follow(drag);
            compare(list.hovered.folder, "/home/ana");
            verify(!drag.accepted, "Backup is already in Home");
            compare(list.action, Drops.NONE);

            drag.y = 50;
            list.follow(drag);
            verify(drag.accepted);
            const dropped = dragOf([], 10, 50);
            list.release(dropped);
            compare(dropped.acceptedWith, Qt.MoveAction);
            compare(drops.count, 1);
            compare(Array.from(drops.signalArguments[0]), [Protocol.TRANSFER_MOVE, ["/home/ana/Backup"], "/home/ana/Photos"]);
            compare(list.hovered, null, "the drag is over");
        }

        function test_dragging_a_folder_into_itself_is_refused() {
            const list = target(listAt);
            const drag = dragOf(["/home/ana/Photos"], 10, 50);
            list.enter(drag);
            verify(drag.accepted, "the list still follows the drag");
            compare(drag.action, Qt.IgnoreAction, "but the pointer says no");
            compare(list.action, Drops.NONE);
            wait(Tokens.durationFilesSpring * 2);
            compare(springs.count, 0, "a refused folder doesn't open");
            const dropped = dragOf([], 10, 50);
            list.release(dropped);
            verify(!dropped.accepted);
            compare(drops.count, 0, "nothing starts");
        }

        function test_dragging_a_folder_onto_the_tree() {
            const rows = [Drops.place("/home/ana", false), Drops.place("/home/ana/Documents", true)];
            const tree = target((x, y) => rows[Math.floor(y / 200)] ?? Drops.NOWHERE);
            const drag = dragOf(["/home/ana/Photos/2025"], 10, 250, Qt.CopyAction);
            tree.enter(drag);
            compare(tree.action, Protocol.TRANSFER_COPY, "Ctrl copies");
            compare(drag.action, Qt.CopyAction);
            const dropped = dragOf([], 10, 250);
            tree.release(dropped);
            compare(Array.from(drops.signalArguments[0]), [Protocol.TRANSFER_COPY, ["/home/ana/Photos/2025"], "/home/ana/Documents"]);
        }

        function test_dragging_a_folder_onto_the_trash() {
            const trash = target(() => Drops.place(Protocol.FILES_TRASH_PLACE, false));
            const drag = dragOf(["/home/ana/Photos", "/home/ana/a.txt"], 10, 10);
            trash.enter(drag);
            compare(drag.action, Qt.MoveAction);
            const dropped = dragOf([], 10, 10);
            trash.release(dropped);
            compare(Array.from(drops.signalArguments[0]), [Drops.TRASH, ["/home/ana/Photos", "/home/ana/a.txt"], Protocol.FILES_TRASH_PLACE]);
            compare(springs.count, 0, "the trash doesn't open");
        }

        function test_a_browser_link_drops_nowhere() {
            const list = target(listAt);
            const drag = dragOf([], 10, 50);
            drag.urls = ["https://example.com/cat.png"];
            list.enter(drag);
            verify(!drag.accepted);
        }
    }
}
