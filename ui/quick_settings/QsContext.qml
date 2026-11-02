import QtQuick
import "quick_settings.js" as QuickSettings

// What quick settings hands each slot it loads (features/<id>/feature.toml
// `quick-settings.*` hosts): where the sidebar is (open, which page), what
// every slot shares (the rings' fill, the clock, the screen), and the
// requests a slot can make of the sidebar. QuickSettings.qml makes one per
// sidebar and answers the signals.
QtObject {
    // The sidebar is open, and the page it shows (quick_settings.js PAGE_*).
    property bool open: false
    property string page: QuickSettings.PAGE_MAIN
    // How far rings are filled in (they fill once it's open).
    property real fill: 1
    // The clock, a minute at a time while it's open.
    property date now: new Date()
    // The screen the sidebar is on.
    property string outputName: ""
    // The room a page's top row leaves on the right for the pill's buttons.
    property real headerInset: 0
    // Where tray menus are placed: `showTrayMenu`'s `at` is in this item.
    property Item menuArea: null

    // Show a page (PAGE_*), or the first one.
    signal pageRequested(string page)
    // Close the sidebar (an action took the user elsewhere).
    signal closeRequested
    // Show a tray app's menu with its middle at `at` (in menuArea), or in
    // the middle when `at` is null.
    signal trayMenuRequested(var entry, var at)
}
