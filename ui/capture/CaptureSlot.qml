import QtQuick
import Quickshell

// The capture.overlay view (shell.overlays), for screenshots and
// recordings: the region picker while it's open, the marks around a region
// being recorded, and the card a saved shot or recording lands in.
Scope {
    CaptureOverlay {}
    ShotCards {}
}
