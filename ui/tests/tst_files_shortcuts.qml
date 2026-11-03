pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../files"
import "../theme"
import "../files/shortcuts.js" as Shortcuts
import "../files/commands.js" as Commands

// Nova Files' keys (shortcuts.js): the whole table as it stands, its rules
// (window keys never edit text, no key means two things in one place), how
// a key press reads as a sequence, typing a name to go to it, and the
// trash's list answering its keys. The root is an Item: a TestCase root
// gets no mouse events.
Item {
    id: stage
    width: 880
    height: 700

    TestCase {
        name: "FilesShortcuts"
        when: windowShown

        // The table as "where: key = action" lines, sorted.
        function lines(): var {
            const all = [];
            for (const entry of Shortcuts.TABLE)
                for (const where of entry.where)
                    for (const key of entry.keys)
                        all.push(`${where}: ${key} = ${entry.action}`);
            return all.sort();
        }

        // Every key Nova Files answers. A change to the table changes this
        // list on purpose.
        function test_the_whole_table() {
            const moves = ["Up", "Down", "PgUp", "PgDown", "Home", "End"];
            const moving = where => moves.concat(moves.map(key => `Shift+${key}`)).map(key => `${where}: ${key} = move`);
            const expected = ["window: Alt+Left = back", "window: Alt+Right = forward", "window: Alt+Up = up", "window: Ctrl+L = address", "window: Alt+D = address", "window: Ctrl+F = search", "window: F3 = search", "window: F5 = refresh", "window: Ctrl+N = new_window", "window: Ctrl+W = close_window", "list: Shift+Return = new_window", "list: Shift+Enter = new_window", "list: Ctrl+Shift+N = new_folder", "list: F2 = rename", "list: Ctrl+X = cut", "list: Ctrl+C = copy", "list: Ctrl+Shift+C = copy_path", "list: Ctrl+V = paste", "list: Alt+Shift+P = details", "list: Backspace = back", "list: Ctrl+H = hidden", "list: Ctrl+A = select_all", "list: Return = open", "list: Enter = open", "list: Alt+Return = properties", "list: Alt+Enter = properties", "list: Del = trash", "list: Shift+Del = delete_for_good", "list: Ctrl+Z = undo", "list: Ctrl+Y = redo", "list: Ctrl+Shift+Z = redo", "trash: Ctrl+Z = undo", "trash: Ctrl+Y = redo", "trash: Ctrl+Shift+Z = redo", "trash: Backspace = back", "trash: Ctrl+A = select_all", "trash: Del = delete_for_good", "trash: Shift+Del = delete_for_good"].concat(moving("list"), moving("trash"));
            compare(lines(), expected.sort());
        }

        function test_window_keys_never_edit_text() {
            for (const entry of Shortcuts.entries(Shortcuts.WINDOW))
                for (const key of entry.keys)
                    verify(!Shortcuts.TEXT_KEYS.includes(key), `${key} edits text`);
        }

        function test_no_key_means_two_things_in_one_place() {
            for (const where of [Shortcuts.WINDOW, Shortcuts.LIST, Shortcuts.TRASH]) {
                const seen = {};
                for (const entry of Shortcuts.entries(where))
                    for (const key of entry.keys) {
                        verify(seen[key] === undefined || seen[key] === entry.action, `${where}: ${key} is ${seen[key]} and ${entry.action}`);
                        seen[key] = entry.action;
                    }
            }
            // A window key that a list had too would never reach the list.
            const windowKeys = [].concat(...Shortcuts.entries(Shortcuts.WINDOW).map(entry => entry.keys));
            for (const where of [Shortcuts.LIST, Shortcuts.TRASH])
                for (const entry of Shortcuts.entries(where))
                    for (const key of entry.keys)
                        verify(!windowKeys.includes(key), `${key} is the window's and the ${where}'s`);
        }

        function test_only_opening_and_closing_windows_work_while_a_dialog_asks() {
            compare(Shortcuts.TABLE.filter(entry => entry.asking).map(entry => entry.action).sort(), [Commands.CLOSE_WINDOW, Commands.NEW_WINDOW]);
        }

        function test_a_key_press_reads_as_its_sequence() {
            compare(Shortcuts.sequence(Qt.Key_N, Qt.ControlModifier | Qt.ShiftModifier), "Ctrl+Shift+N");
            compare(Shortcuts.sequence(Qt.Key_F5, Qt.NoModifier), "F5");
            compare(Shortcuts.sequence(Qt.Key_Delete, Qt.ShiftModifier), "Shift+Del");
            compare(Shortcuts.sequence(Qt.Key_PageDown, Qt.NoModifier), "PgDown");
            compare(Shortcuts.sequence(Qt.Key_2, Qt.ControlModifier), "Ctrl+2");
            compare(Shortcuts.sequence(Qt.Key_Left, Qt.AltModifier), "Alt+Left");
            compare(Shortcuts.sequence(Qt.Key_Shift, Qt.ShiftModifier), "", "a modifier on its own");
        }

        function test_keys_do_their_place_s_action() {
            compare(Shortcuts.action(Shortcuts.LIST, Qt.Key_Delete, Qt.NoModifier), Commands.TRASH);
            compare(Shortcuts.action(Shortcuts.LIST, Qt.Key_Delete, Qt.ShiftModifier), Commands.DELETE_FOR_GOOD);
            compare(Shortcuts.action(Shortcuts.TRASH, Qt.Key_Delete, Qt.NoModifier), Commands.DELETE_FOR_GOOD);
            compare(Shortcuts.action(Shortcuts.LIST, Qt.Key_End, Qt.ShiftModifier), Commands.MOVE);
            compare(Shortcuts.action(Shortcuts.LIST, Qt.Key_Return, Qt.ShiftModifier), Commands.NEW_WINDOW);
            compare(Shortcuts.action(Shortcuts.LIST, Qt.Key_N, Qt.ControlModifier | Qt.ShiftModifier), Commands.NEW_FOLDER);
            compare(Shortcuts.action(Shortcuts.LIST, Qt.Key_X, Qt.NoModifier), "", "a plain letter is typing");
            compare(Shortcuts.action(Shortcuts.TRASH, Qt.Key_Return, Qt.NoModifier), "", "a trashed item doesn't open");
            compare(Shortcuts.action(Shortcuts.LIST, Qt.Key_H, Qt.ControlModifier | Qt.ShiftModifier), "", "held keys count");
            compare(Shortcuts.action(Shortcuts.WINDOW, Qt.Key_F, Qt.ControlModifier), Commands.SEARCH);
        }

        function test_menus_show_a_command_s_key() {
            compare(Shortcuts.shown(Commands.COPY_PATH), "Ctrl + Shift + C");
            compare(Shortcuts.shown(Commands.TRASH), "Delete");
            compare(Shortcuts.shown(Commands.NEW_WINDOW), "Shift + Enter", "a list's key before the window's");
            compare(Shortcuts.shown(Commands.REFRESH), "F5");
            compare(Shortcuts.shown(Commands.OPEN_WITH), "", "no key, nothing shown");
        }

        function test_only_printable_text_without_ctrl_or_alt_types() {
            compare(Shortcuts.typed("d", Qt.NoModifier, ""), "d");
            compare(Shortcuts.typed("D", Qt.ShiftModifier, ""), "D");
            compare(Shortcuts.typed("d", Qt.ControlModifier, ""), "");
            compare(Shortcuts.typed("d", Qt.AltModifier, ""), "");
            compare(Shortcuts.typed("\b", Qt.NoModifier, ""), "", "Backspace isn't typing");
            compare(Shortcuts.typed("", Qt.NoModifier, ""), "");
            compare(Shortcuts.typed(" ", Qt.NoModifier, ""), "", "a space starts no name");
            compare(Shortcuts.typed(" ", Qt.NoModifier, "my"), " ", "but continues one");
        }

        function test_typing_a_name_goes_to_it() {
            const names = ["Desktop", "Documents", "Downloads", "music", "notes.md"];
            compare(Shortcuts.find(names, -1, "m"), 3, "case doesn't matter");
            compare(Shortcuts.find(names, 0, "d"), 1, "the same letter steps to the next");
            compare(Shortcuts.find(names, 2, "d"), 0, "and wraps round");
            compare(Shortcuts.find(names, 2, "dd"), 0, "however often it's typed");
            compare(Shortcuts.find(names, 1, "do"), 1, "a longer name stays while it matches");
            compare(Shortcuts.find(names, 1, "dow"), 2);
            compare(Shortcuts.find(names, 0, "x"), -1);
            compare(Shortcuts.find([], -1, "a"), -1);
        }

        Component {
            id: typingComponent
            NameTyping {}
        }

        function test_a_pause_starts_a_new_name() {
            const typing = createTemporaryObject(typingComponent, stage);
            const names = createTemporaryQmlObject("import QtQuick; ListModel {}", stage);
            for (const name of ["alpha", "beta", "bravo"])
                names.append({
                    name: name
                });
            const went = [];
            const go = index => went.push(index);
            verify(typing.press("b", Qt.NoModifier, names, 0, go));
            verify(typing.press("r", Qt.NoModifier, names, 1, go));
            compare(went, [1, 2]);
            verify(!typing.press("a", Qt.ControlModifier, names, 2, go), "Ctrl+A isn't typing");
            compare(typing.text, "br");
            tryCompare(typing, "text", "", Tokens.filesTypingPause * 2);
            verify(typing.press("a", Qt.NoModifier, names, 2, go));
            compare(went, [1, 2, 0]);
        }

        // A trashed item as novad lists it.
        function trashed(id, name) {
            return {
                id: id,
                folder: "/home/ana",
                deleted_ms: Date.UTC(2026, 9, 30, 9, 0),
                entry: {
                    name: name,
                    dir: false,
                    link: false,
                    hidden: false,
                    mime: "text/plain",
                    size: 4096,
                    modified_ms: Date.UTC(2026, 9, 31, 9, 12)
                }
            };
        }

        Component {
            id: trashModelComponent
            TrashModel {}
        }

        Component {
            id: trashListComponent
            TrashList {
                width: 640
                height: 400
                home: "/home/ana"
            }
        }

        SignalSpy {
            id: actions
            signalName: "commandRequested"
        }

        SignalSpy {
            id: erases
            signalName: "eraseRequested"
        }

        function test_the_trash_answers_its_keys() {
            const model = createTemporaryObject(trashModelComponent, stage);
            model._fill([trashed("a", "apples.txt"), trashed("b", "bills.pdf"), trashed("c", "budget.ods")]);
            const list = createTemporaryObject(trashListComponent, stage, {
                model: model
            });
            actions.target = list;
            erases.target = list;
            actions.clear();
            erases.clear();
            list.focusList();
            keyClick(Qt.Key_Delete);
            compare(erases.count, 0, "nothing picked, nothing to delete");
            keyClick(Qt.Key_B);
            compare(model.selectedRows().map(row => row.name), ["bills.pdf"]);
            keyClick(Qt.Key_B);
            compare(model.selectedRows().map(row => row.name), ["budget.ods"], "b again steps on");
            keyClick(Qt.Key_Home, Qt.ShiftModifier);
            compare(model.selectedCount, 3, "Shift+Home extends to the top");
            keyClick(Qt.Key_Delete, Qt.ShiftModifier);
            compare(erases.count, 1);
            keyClick(Qt.Key_Backspace);
            compare(actions.signalArguments.map(args => args[0]), [Commands.BACK]);
            keyClick(Qt.Key_Down);
            compare(model.selectedCount, 1);
            keyClick(Qt.Key_A, Qt.ControlModifier);
            compare(model.selectedCount, 3);
            // The list goes before its model, as in a window.
            list.destroy();
            wait(0);
        }
    }
}
