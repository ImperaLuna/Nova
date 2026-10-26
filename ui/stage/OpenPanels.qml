pragma Singleton

import QtQuick

// Where StagePanels on every screen hear that one of them opened, so each
// can close what the open one rules out (StagePanel says what that is), and
// that there was a click outside the panels on some screen, so each closes
// those that close on it.
QtObject {
    // `panel` (a StagePanel) has just opened.
    signal opened(Item panel)
    // Esc, or a click or scroll off the panels on any screen.
    signal dismissed
}
