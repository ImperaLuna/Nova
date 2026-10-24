pragma Singleton

import QtQuick

// Where StagePanels on every screen hear that one of them opened, so each
// can close what the open one rules out (StagePanel says what that is).
QtObject {
    // `panel` (a StagePanel) has just opened.
    signal opened(Item panel)
}
