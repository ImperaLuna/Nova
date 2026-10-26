pragma Singleton

import QtQuick
import Quickshell
import "../text/files.js" as Files

// What an Image shows for an app's or a notification's icon: the absolute
// path novad sends, as a file URL. novad looks icon names up in the icon
// theme, off the GUI thread; Qt's own lookup (Quickshell.iconPath) blocks
// it for 30-150 ms per name the first time (IMP-158). "" for no icon.
Singleton {
    function source(icon: string): string {
        return icon.startsWith("/") ? Files.fileUrl(icon) : "";
    }
}
