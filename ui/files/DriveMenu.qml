import QtQuick
import "../theme/icons.js" as Icons

// A drive's right-click menu, at the pointer, over the whole window: a
// click beside it closes it. Eject is only here, never a button on the
// drive, so it can't be hit by accident; it shows for drives that can be
// unplugged.
PopupMenu {
    id: menu

    property var drive: null

    signal ejectChosen(var drive)

    function open(drive: var, at: point) {
        menu.drive = drive;
        place(at);
    }

    function close() {
        drive = null;
    }

    shown: drive !== null && drive.removable
    onDismissed: close()

    MenuRow {
        icon: Icons.EJECT
        label: qsTr("Eject")
        onChosen: {
            const drive = menu.drive;
            menu.close();
            menu.ejectChosen(drive);
        }
    }
}
