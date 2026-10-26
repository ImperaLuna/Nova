pragma Singleton

import QtQuick
import Quickshell
import "../text/files.js" as Files

// What an Image shows for an app's or a notification's icon: an icon
// theme name looked up in the theme, or an absolute path to an image as
// a file URL. "" when there's no icon or the theme has none.
Singleton {
    function source(icon: string): string {
        if (!icon)
            return "";
        if (icon.startsWith("/"))
            return Files.fileUrl(icon);
        return Quickshell.iconPath(icon, true);
    }
}
