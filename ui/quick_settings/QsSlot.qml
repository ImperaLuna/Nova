import QtQuick

// The root of every view quick settings loads into a slot: a thin adapter
// that reads its feature's service, passes properties to a view that only
// takes properties (for ui/tests), and acts through the service or `qs`.
Item {
    // The sidebar this slot is in.
    required property QsContext qs
    // Whether there's something to show now (a card with no devices isn't):
    // the host leaves the slot out while false.
    property bool present: true
    // Still settling after a change: the sidebar keeps drawing until done.
    property bool running: false
}
