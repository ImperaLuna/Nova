pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../theme"
import "../theme/icons.js" as Icons
import "home.js" as Home
import "paths.js" as Paths
import "service"

// Nova Files' Home page, where a new window opens, as Explorer's Home: the
// folders pinned to Quick access and the drives as tiles, then the files
// any app used last (Recent) as a list, each section under its heading and
// left out while it has nothing. One thing on the page is selected at a
// time: a click selects it, a double-click or Enter opens it (a folder or a
// drive in the window, a file in its app). A right-click on a pinned
// folder asks for its pin menu, on a drive for the drive's menu. Recent is
// read again each time the page shows.
FocusScope {
    id: page

    required property Navigator navigator
    // How solid the list's background is (0..1), as the file list's.
    property real solid: 0

    // The Recent rows (Home.recentRow), the latest first.
    property var recent: []
    // What's selected: "pin:", "drive:" or "recent:" and its path or id;
    // "" for nothing.
    property string selected: ""
    // When Recent's Date used was worked out, so "Today" moves on.
    property real now: Date.now()
    readonly property string home: Quickshell.env("HOME") ?? ""
    readonly property var pinned: Home.pinned(FilesService.places)

    signal pinMenuRequested(string path, point at)
    signal driveMenuRequested(var drive, point at)

    function refresh() {
        FilesService.recent(files => {
            // A failed read keeps what's shown.
            if (files !== null)
                page.recent = files.map(file => Home.recentRow(file));
            page.now = Date.now();
        });
    }

    // Opens what `key` (a `selected` value) names.
    function open(key: string) {
        const at = key.indexOf(":");
        const kind = key.slice(0, at);
        const value = key.slice(at + 1);
        if (kind === "pin") {
            navigator.visit(value, null);
        } else if (kind === "drive") {
            navigator.openDrive(value);
        } else if (kind === "recent") {
            const file = recent.find(row => Paths.child(row.folder, row.name) === value);
            if (file?.dir)
                navigator.visit(value, null);
            else if (file)
                FilesService.launch(value, "", null);
        }
    }

    // Quick access is read again too: another app may have pinned since.
    onVisibleChanged: {
        if (visible) {
            selected = "";
            refresh();
            FilesService.refreshPlaces();
        }
    }
    Component.onCompleted: {
        if (visible) {
            refresh();
            forceActiveFocus();
        }
    }

    Keys.onPressed: event => {
        if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && selected !== "") {
            open(selected);
            event.accepted = true;
        }
    }
    Keys.onEscapePressed: selected = ""

    Timer {
        interval: Tokens.filesClockTick
        running: page.visible
        repeat: true
        onTriggered: page.now = Date.now()
    }

    Rectangle {
        anchors.fill: parent
        color: Tokens.mix(Tokens.glass, Tokens.filesSolidList, page.solid)
    }

    component ColumnName: Text {
        anchors.verticalCenter: parent?.verticalCenter
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeSmall
        color: Tokens.filesFaint
        elide: Text.ElideRight
    }
    component Heading: Text {
        topPadding: Tokens.filesHomeSectionGap
        bottomPadding: Tokens.gap
        leftPadding: Tokens.rowInset
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeHeading
        font.weight: Tokens.weightTitle
        color: Tokens.fg
    }

    Flickable {
        id: scroller
        anchors.fill: parent
        anchors.margins: Tokens.filesHomePad
        anchors.topMargin: Tokens.filesHomePad - Tokens.filesHomeSectionGap
        contentHeight: sections.implicitHeight
        boundsBehavior: Flickable.StopAtBounds
        clip: true

        // A click on nothing in particular clears the selection.
        MouseArea {
            width: scroller.width
            height: Math.max(scroller.height, sections.implicitHeight)
            onPressed: {
                page.selected = "";
                page.forceActiveFocus();
            }
        }

        Column {
            id: sections
            width: scroller.width

            Heading {
                visible: page.pinned.length > 0
                text: qsTr("Quick access")
            }
            Flow {
                width: parent.width
                spacing: Tokens.gap

                Repeater {
                    model: page.pinned

                    HomeTile {
                        required property var modelData
                        readonly property string key: "pin:" + modelData.path
                        readonly property var look: Home.look(modelData)

                        icon: look.icon
                        iconColor: Tokens.filesFolder
                        title: look.name
                        detail: Paths.located(Paths.parent(modelData.path), page.home)
                        selected: page.selected === key
                        onPicked: {
                            page.selected = key;
                            page.forceActiveFocus();
                        }
                        onOpened: page.open(key)
                        onMenuRequested: at => page.pinMenuRequested(modelData.path, at)
                    }
                }
            }

            Heading {
                visible: FilesService.drives.length > 0
                text: qsTr("Drives")
            }
            Flow {
                width: parent.width
                spacing: Tokens.gap

                Repeater {
                    model: FilesService.drives

                    HomeTile {
                        required property var modelData
                        readonly property string key: "drive:" + modelData.id

                        icon: modelData.removable ? Icons.USB : Icons.DISK
                        title: modelData.name
                        detail: Home.free(modelData)
                        measured: Home.measured(modelData)
                        fill: Home.fill(modelData)
                        selected: page.selected === key
                        onPicked: {
                            page.selected = key;
                            page.forceActiveFocus();
                        }
                        onOpened: page.open(key)
                        onMenuRequested: at => page.driveMenuRequested(modelData, at)
                    }
                }
            }

            Heading {
                visible: page.recent.length > 0
                text: qsTr("Recent")
            }
            Row {
                id: columns
                visible: page.recent.length > 0
                width: parent.width
                height: Tokens.filesHeaderHeight
                leftPadding: Tokens.rowInset
                rightPadding: Tokens.rowInset
                spacing: Tokens.rowInset

                ColumnName {
                    width: columns.width - columns.leftPadding - columns.rightPadding - Tokens.filesRecentColumns[0] - Tokens.filesRecentColumns[1] - 2 * columns.spacing
                    leftPadding: Tokens.iconSize + Tokens.rowInset
                    text: qsTr("Name")
                }
                ColumnName {
                    width: Tokens.filesRecentColumns[0]
                    text: qsTr("Date used")
                }
                ColumnName {
                    width: Tokens.filesRecentColumns[1]
                    text: qsTr("Location")
                }
            }
            Repeater {
                model: page.recent

                RecentRow {
                    required property var modelData
                    readonly property string key: "recent:" + Paths.child(modelData.folder, modelData.name)

                    width: sections.width
                    file: modelData
                    now: page.now
                    home: page.home
                    selected: page.selected === key
                    onPicked: {
                        page.selected = key;
                        page.forceActiveFocus();
                    }
                    onOpened: page.open(key)
                }
            }
        }
    }
}
