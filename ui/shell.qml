//@ pragma UseQApplication

import QtQuick
import Quickshell
import "bar"
import "daemon"

ShellRoot {
    Variants {
        // Nothing is drawn until novad has sent the theme's colors.
        model: Daemon.theme ? Quickshell.screens : []

        Bar {
            required property var modelData
            screen: modelData
        }
    }
}
