import QtQuick
import "../theme/icons.js" as Icons

// A drive's right-click menu, at the pointer, over the whole window: a
// click beside it closes it. Properties, for a mounted drive, and Eject.
// Eject is only here, never a button on the drive, so it can't be hit by
// accident; it shows for drives that can be unplugged.
PopupMenu {
    id: menu

    property var drive: null
    readonly property bool mounted: drive !== null && drive.mount !== undefined

    signal ejectChosen(var drive)
    signal propertiesChosen(var drive)

    function open(drive: var, at: point) {
        menu.drive = drive;
        place(at);
    }

    function close() {
        drive = null;
    }

    shown: drive !== null && (drive.removable || mounted)
    onDismissed: close()

    MenuRow {
        visible: menu.mounted
        icon: Icons.DISK
        label: qsTr("Properties")
        onChosen: {
            const drive = menu.drive;
            menu.close();
            menu.propertiesChosen(drive);
        }
    }
    MenuRow {
        visible: menu.drive?.removable ?? false
        icon: Icons.EJECT
        label: qsTr("Eject")
        onChosen: {
            const drive = menu.drive;
            menu.close();
            menu.ejectChosen(drive);
        }
    }
}
