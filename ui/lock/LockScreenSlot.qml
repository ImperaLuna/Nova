import QtQuick
import Quickshell
import "service"

// The lock.screen view (shell.overlays): the lock screen, held while the
// session is locked, so turning the feature off then can't drop the
// session lock and leave the screens covered for good.
Scope {
    readonly property bool held: LockService.locked

    LockScreen {}
}
