pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "service"

// The lock screen: the compositor's session lock (ext-session-lock) with a
// LockSurface on every screen, unlocked by the Authenticator's password
// check. LockService says when to lock. If the UI dies while locked, the
// compositor keeps the screens covered, and the restarted UI locks again.
Scope {
    id: root

    Authenticator {
        id: passwordCheck
    }

    WlSessionLock {
        id: lock

        locked: LockService.locked

        // The VM tests wait for these.
        onSecureChanged: {
            if (secure)
                console.info("nova: session locked");
        }
        onLockedChanged: {
            if (!locked)
                console.info("nova: session unlocked");
        }

        LockSurface {
            authenticator: passwordCheck
        }
    }
}
