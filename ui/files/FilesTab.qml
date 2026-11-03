import QtQml
import "paths.js" as Paths
import "trash.js" as Trash

// One tab of a Nova Files window: where it is and has been (its own
// Navigator, so back and forward stay with the tab), and how its file list
// looked as it went to the back (the order and the selected names), so
// coming back finds it as it was. The window shows one tab at a time in
// its one file list.
QtObject {
    id: tab

    readonly property Navigator navigator: Navigator {}
    // FilesWindow.snapshot() as the tab last went to the back, or null
    // until it first did.
    property var view: null
    readonly property bool trash: Trash.isTrash(navigator.folder)
    // The tab's name: the folder's, or the trash's.
    readonly property string title: trash ? Trash.name() : Paths.folderName(navigator.folder)
}
