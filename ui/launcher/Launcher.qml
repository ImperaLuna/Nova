pragma ComponentBehavior: Bound

import QtQuick
import "../stage"
import "../theme"
import "../daemon/protocol.js" as Protocol
import "launcher.js" as LauncherLogic

// The launcher: a glass deck near the top of the middle of the screen that
// fades in where it stands (the central pill stays). A search field, the
// pinned apps as tiles, and a list: the recent apps (every app before the
// first launch), or what a search matches. Up and Down move through the
// list, Enter launches (bringing an open app's window here; Shift+Enter
// opens another), Esc or a click outside closes. Opened by
// `nova toggle launcher` (Super+Space) or the central pill's search button.
// It gets its data and acts through its properties and signals (the stage
// wires LauncherService to them), so the QML tests can drive it.
StagePanel {
    id: launcher

    // The screen's size, which it centers itself in.
    required property real screenWidth
    required property real screenHeight
    // A launcher event's data: every app, and the pinned and recent ids.
    property var apps: []
    property var pinned: []
    property var recent: []
    // An app's icon (its theme name or path) to an Image source, "" for none.
    property var iconSource: icon => ""

    // Typed: novad's matches go to showMatches().
    signal searchRequested(string query)
    // An app to start, or to bring here when it's open already unless
    // `newWindow` (Shift+Enter): the answer goes to launched().
    signal launchRequested(string id, bool newWindow)
    signal pinRequested(string id, bool pinned)

    // What's typed, and the matches novad sent for it.
    property string query: ""
    property var matches: []
    // The selected row of the list.
    property int selected: 0
    // The app starting right now: its row turns the accent until it has.
    property string launching: ""
    // Why the last launch failed, over the list.
    property string problem: ""

    readonly property var known: LauncherLogic.byId(apps)
    readonly property var rows: LauncherLogic.rows(query, matches, recent, apps, known)
    readonly property real contentWidth: Tokens.launcherWidth - 2 * Tokens.launcherPad
    // As many tiles as fit on one row, spread to fill it.
    readonly property int tilesPerRow: Math.floor((contentWidth + Tokens.gap) / (Tokens.launcherTileSize + Tokens.gap))
    readonly property var pinnedApps: LauncherLogic.pick(known, pinned).slice(0, tilesPerRow)
    readonly property bool showsPinned: query.trim() === "" && pinnedApps.length > 0

    name: Protocol.FEATURE_LAUNCHER
    placement: "center"
    wantsKeyboard: true
    radius: Tokens.radiusPanel
    // Unblurred once it starts closing: a compositor's blur can't fade, so it
    // would stay a blurred box while the deck fades out over it.
    glass: open

    readonly property Morph morph: Morph {
        open: launcher.open
    }
    moving: morph.moving || resize.running || closing.running

    // Open, it's as tall as what it shows, changing size smoothly as that
    // changes; its surface is as big as it can get, so that never resizes it.
    readonly property real openY: Math.round(screenHeight * Tokens.launcherTop)
    // The list: launcherRows rows and the headings of the two groups they may
    // span (recent, then every app) before it scrolls.
    readonly property real listHeight: Tokens.launcherRows * Tokens.launcherRowHeight + (Tokens.launcherRows - 1) * Tokens.launcherRowGap + 2 * Tokens.launcherLabelHeight
    readonly property real tallest: 2 * Tokens.launcherPad + Tokens.launcherSearchHeight + 4 * Tokens.launcherGap + 2 * Tokens.launcherLabelHeight + Tokens.launcherTileSize + Tokens.launcherTileName + listHeight
    property real shownHeight: content.implicitHeight + 2 * Tokens.launcherPad
    Behavior on shownHeight {
        enabled: launcher.open && !launcher.morph.moving
        NumberAnimation {
            id: resize
            duration: Tokens.durationResize
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }
    // It fades in a little smaller and higher than its place.
    readonly property real grow: morph.lerp(Tokens.launcherEnterScale, 1)

    width: Tokens.launcherWidth * grow
    height: shownHeight * grow
    x: (screenWidth - width) / 2
    y: openY - morph.lerp(Tokens.launcherEnterRise, 0)
    opacity: morph.t
    area: Qt.rect((screenWidth - Tokens.launcherWidth) / 2, openY - Tokens.launcherEnterRise, Tokens.launcherWidth, tallest + Tokens.launcherEnterRise)

    onOpenChanged: {
        console.info(`nova: launcher ${open ? "opened" : "closed"}`);
        if (!open)
            return;
        field.text = "";
        matches = [];
        selected = 0;
        launching = "";
        problem = "";
        field.forceActiveFocus();
    }

    onRowsChanged: selected = Math.min(selected, Math.max(0, rows.length - 1))
    onSelectedChanged: list.positionViewAtIndex(selected, ListView.Contain)

    // novad's matches for `forQuery`; dropped when something else has been
    // typed since.
    function showMatches(forQuery: string, found: var) {
        if (forQuery === query)
            matches = found;
    }

    // Starts the app `id` (see launchRequested), unless one is starting
    // already.
    function launch(id: string, newWindow: bool) {
        if (launching !== "")
            return;
        problem = "";
        launching = id;
        launchRequested(id, newWindow);
    }

    // novad's answer to launchRequested: closes once the app has started.
    function launched(error: var) {
        if (error) {
            problem = error;
            launching = "";
            return;
        }
        closing.start();
    }

    // A key typed into the search field; true when it was the launcher's.
    function key(key: int, modifiers: int): bool {
        switch (key) {
        case Qt.Key_Up:
        case Qt.Key_Down:
            selected = LauncherLogic.step(selected, rows.length, key === Qt.Key_Up ? -1 : 1);
            return true;
        case Qt.Key_Return:
        case Qt.Key_Enter:
            if (rows.length > 0)
                launch(rows[selected].id, (modifiers & Qt.ShiftModifier) !== 0);
            return true;
        case Qt.Key_Escape:
            open = false;
            return true;
        }
        return false;
    }

    // The launching row shows the accent this long before the deck goes.
    Timer {
        id: closing
        interval: Tokens.durationFast
        onTriggered: launcher.open = false
    }

    Glass {
        anchors.fill: parent
        radius: launcher.radius
    }

    // Laid out at full size and scaled with the deck as it fades in.
    Item {
        width: Tokens.launcherWidth
        height: launcher.shownHeight
        scale: launcher.grow
        transformOrigin: Item.TopLeft
        clip: true

        Column {
            id: content
            x: Tokens.launcherPad
            y: Tokens.launcherPad
            width: launcher.contentWidth
            spacing: Tokens.launcherGap

            Rectangle {
                width: parent.width
                height: Tokens.launcherSearchHeight
                radius: Tokens.radiusRow
                color: Tokens.surfaceRaised

                LauncherIcon {
                    id: searchIcon
                    anchors.verticalCenter: parent.verticalCenter
                    x: Tokens.launcherPad
                    width: Tokens.iconSizeMenu
                    height: Tokens.iconSizeMenu
                    name: "search"
                    color: Tokens.fgDim
                }

                TextInput {
                    id: field
                    anchors.verticalCenter: parent.verticalCenter
                    x: searchIcon.x + searchIcon.width + Tokens.launcherGap
                    width: escHint.x - x - Tokens.launcherGap
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSearch
                    font.weight: Font.Medium
                    color: Tokens.fg
                    selectionColor: Tokens.primary
                    selectedTextColor: Tokens.onAccent
                    clip: true
                    maximumLength: Protocol.SEARCH_APPS_MAX_CHARS
                    cursorDelegate: Rectangle {
                        width: Tokens.caretWidth
                        color: Tokens.primary
                        visible: field.cursorVisible
                    }
                    onTextChanged: {
                        launcher.query = text;
                        launcher.selected = 0;
                        launcher.problem = "";
                        if (text.trim() !== "")
                            launcher.searchRequested(text);
                        else
                            launcher.matches = [];
                    }
                    Keys.onPressed: event => {
                        event.accepted = launcher.key(event.key, event.modifiers);
                    }

                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        visible: field.text === ""
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeSearch
                        color: Tokens.fgDim
                        text: qsTr("Search apps")
                    }
                }

                Text {
                    id: escHint
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.right: parent.right
                    anchors.rightMargin: Tokens.launcherPad
                    font.family: Tokens.fontMono
                    font.pixelSize: Tokens.fontSizeTiny
                    color: Tokens.fgDim
                    text: qsTr("Esc")
                }
            }

            SectionLabel {
                visible: launcher.showsPinned
                text: qsTr("Pinned")
            }

            Row {
                visible: launcher.showsPinned
                spacing: launcher.tilesPerRow > 1 ? (launcher.contentWidth - launcher.tilesPerRow * Tokens.launcherTileSize) / (launcher.tilesPerRow - 1) : 0

                Repeater {
                    model: launcher.pinnedApps

                    AppTile {
                        required property var modelData
                        appId: modelData.id
                        name: modelData.name
                        iconSource: launcher.iconSource(modelData.icon ?? "")
                        onClicked: launcher.launch(appId, false)
                    }
                }
            }

            // Why the last launch failed.
            SectionLabel {
                visible: launcher.problem !== ""
                width: parent.width
                rightPadding: Tokens.rowInset
                elide: Text.ElideRight
                font.capitalization: Font.MixedCase
                color: Tokens.error
                textFormat: Text.PlainText
                text: qsTr("Couldn't start it: %1").arg(launcher.problem)
            }

            ListView {
                id: list
                objectName: "list"
                width: parent.width
                height: Math.min(contentHeight, launcher.listHeight)
                visible: count > 0
                spacing: Tokens.launcherRowGap
                clip: true
                interactive: contentHeight > height
                boundsBehavior: Flickable.StopAtBounds
                model: launcher.rows
                currentIndex: launcher.selected
                // Behind the selected row, not its group's heading above it.
                highlightFollowsCurrentItem: false

                highlight: Rectangle {
                    readonly property LauncherListItem current: list.currentItem as LauncherListItem
                    y: current ? current.y + current.rowY : 0
                    width: list.width
                    height: Tokens.launcherRowHeight
                    radius: Tokens.radiusRow
                    Behavior on y {
                        NumberAnimation {
                            duration: Tokens.durationFast
                            easing.type: Easing.BezierSpline
                            easing.bezierCurve: Tokens.easing
                        }
                    }
                    color: launcher.launching !== "" ? Qt.rgba(Tokens.primary.r, Tokens.primary.g, Tokens.primary.b, Tokens.accentFillOpacity) : Tokens.selected
                    Behavior on color {
                        ColorAnimation {
                            duration: Tokens.durationFast
                        }
                    }
                }

                delegate: LauncherListItem {
                    width: list.width
                }
            }

            Text {
                width: parent.width
                height: Tokens.launcherEmptyHeight
                visible: launcher.query.trim() !== "" && launcher.rows.length === 0
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                color: Tokens.fgDim
                elide: Text.ElideMiddle
                textFormat: Text.PlainText
                text: qsTr("Nothing matches “%1”").arg(launcher.query.trim())
            }
        }
    }

    // A row, under its group's heading when it's the group's first.
    component LauncherListItem: Column {
        id: item

        required property var modelData
        required property int index
        readonly property bool heads: index === 0 || launcher.rows[index - 1]?.group !== modelData.group
        // Where the row starts, below the heading.
        readonly property real rowY: heads ? Tokens.launcherLabelHeight : 0

        SectionLabel {
            visible: item.heads
            text: ({
                    recent: qsTr("Recent"),
                    apps: qsTr("All apps"),
                    results: qsTr("Results")
                })[item.modelData.group] ?? ""
        }

        LauncherRow {
            width: item.width
            app: item.modelData
            iconSource: launcher.iconSource(item.modelData.icon)
            selected: item.index === launcher.selected
            pinned: launcher.pinned.includes(item.modelData.id)
            launching: launcher.launching === item.modelData.id
            onHovered: launcher.selected = item.index
            onClicked: launcher.launch(item.modelData.id, false)
            onPinClicked: launcher.pinRequested(item.modelData.id, !pinned)
        }
    }

    component SectionLabel: Text {
        height: Tokens.launcherLabelHeight
        leftPadding: Tokens.rowInset
        verticalAlignment: Text.AlignVCenter
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeTiny
        font.capitalization: Font.AllUppercase
        color: Tokens.fgDim
    }
}
