pragma ComponentBehavior: Bound

import QtQuick
import "../stage"
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "launcher.js" as LauncherLogic
import "../files/paths.js" as Paths

// The launcher: a glass deck near the top of the middle of the screen that
// fades in where it stands (the central pill stays). A search field, the
// pinned apps as tiles, and a list: the recent apps (every app before the
// first launch), or what a search's providers find (apps first: the
// launcher.providers slot order). Up and Down move through the
// list, Enter launches (focusing an open app's window where it is;
// Shift+Enter opens another), Esc or a click outside closes. Opened by
// `nova toggle launcher` (Super+Space) or the central pill's search button.
// With Nova Files on, tabs in the field switch to the Files mode (or
// Ctrl+Tab, or typing `~` or `/` first): files found by name, or a typed
// path's folder, whose completion shows grey and Tab takes; Enter opens a
// file in its app or a folder in Nova Files, Shift+Enter shows it in its
// folder.
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

    // Nova Files runs: the Files mode is there.
    property bool filesMode: false
    // The mode: Protocol.PROVIDER_APPS, or PROVIDER_FILES.
    property string mode: Protocol.PROVIDER_APPS
    readonly property bool inFiles: mode === Protocol.PROVIDER_FILES
    // Nova Files stopping takes its mode with it.
    onFilesModeChanged: {
        if (!filesMode)
            setMode(Protocol.PROVIDER_APPS);
    }

    // Typed, in `mode`: novad's matches go to showMatches().
    signal searchRequested(string query, string mode)
    // What `provider` found as `id` to run: an app starts, or is focused
    // where it is when it's open already unless `newWindow` (Shift+Enter).
    // The answer goes to launched().
    signal runRequested(string provider, string id, bool newWindow)
    signal pinRequested(string id, bool pinned)

    // What's typed, and what novad's providers found for it (LauncherItems).
    property string query: ""
    property var matches: []
    // The query `matches` answer; until novad answers the latest, they're
    // the previous query's.
    property string matchesFor: ""
    // Enter pressed before novad answered the latest query: launches its
    // first match when it does ({newWindow}), or null.
    property var enterPending: null
    // The selected row of the list.
    property int selected: 0
    // The row running right now ({provider, id}, or null): it turns the
    // accent until it has.
    property var launching: null
    // Why the last launch failed, over the list.
    property string problem: ""

    readonly property var known: LauncherLogic.byId(apps)
    readonly property var rows: LauncherLogic.rows(query, matches, recent, apps, known, inFiles)
    // What Tab adds to the query: the selected file's path, past what's typed.
    // Only once novad has answered what's typed: older rows complete an older query.
    readonly property string completion: inFiles && matchesFor === query ? LauncherLogic.completion(query, rows[selected]) : ""
    // Typing a path switched to the Files mode, which clearing the field undoes.
    property bool pathSwitched: false
    readonly property real contentWidth: Tokens.launcherWidth - 2 * Tokens.launcherPad
    // As many tiles as fit on one row, spread to fill it.
    readonly property int tilesPerRow: Math.floor((contentWidth + Tokens.gap) / (Tokens.launcherTileSize + Tokens.gap))
    readonly property var pinnedApps: LauncherLogic.pick(known, pinned).slice(0, tilesPerRow)
    readonly property bool showsPinned: !inFiles && query.trim() === "" && pinnedApps.length > 0

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
    // On a short screen, fewer: the deck ends a pad above the screen's edge.
    readonly property real chrome: 2 * Tokens.launcherPad + Tokens.launcherSearchHeight + 4 * Tokens.launcherGap + 2 * Tokens.launcherLabelHeight + Tokens.launcherTileSize + Tokens.launcherTileName
    readonly property real listHeight: Math.max(Tokens.launcherRowHeight, Math.min(Tokens.launcherRows * Tokens.launcherRowHeight + (Tokens.launcherRows - 1) * Tokens.launcherRowGap + 2 * Tokens.launcherLabelHeight, screenHeight - openY - chrome - Tokens.launcherPad))
    readonly property real tallest: chrome + listHeight
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
        // An Enter waiting for matches is dropped with the deck.
        enterPending = null;
        if (!open)
            return;
        mode = Protocol.PROVIDER_APPS;
        field.text = "";
        matches = [];
        matchesFor = "";
        enterPending = null;
        selected = 0;
        launching = null;
        problem = "";
        field.forceActiveFocus();
    }

    onRowsChanged: selected = Math.min(selected, Math.max(0, rows.length - 1))
    onSelectedChanged: list.positionViewAtIndex(selected, ListView.Contain)

    // Where the pointer last was over a row. The list scrolling under a
    // still pointer moves rows under it too, which mustn't take the
    // selection from the arrow keys.
    property point pointer: Qt.point(-1, -1)

    // The pointer is over row `index`, at `at`: selects it if it moved there.
    function pointed(index: int, at: point) {
        if (at.x === pointer.x && at.y === pointer.y)
            return;
        pointer = at;
        selected = index;
    }

    // novad's matches for `forQuery` in `forMode` (this mode when unset);
    // dropped when something else has been typed or picked since.
    function showMatches(forQuery: string, found: var, forMode: var) {
        if (forQuery !== query || (forMode ?? mode) !== mode)
            return;
        matches = found;
        matchesFor = forQuery;
        const pending = enterPending;
        enterPending = null;
        if (pending && rows.length > 0)
            launch(rows[0], pending.newWindow);
    }

    // Switches to the mode `next` (see `mode`), searching what's typed in it.
    function setMode(next: string) {
        pathSwitched = false;
        if (next === mode || (next === Protocol.PROVIDER_FILES && !filesMode))
            return;
        mode = next;
        matches = [];
        matchesFor = "";
        enterPending = null;
        selected = 0;
        problem = "";
        if (query.trim() !== "")
            searchRequested(query, mode);
    }

    // Runs the row `row` ({provider, id}; see runRequested), unless one is
    // running already.
    function launch(row: var, newWindow: bool) {
        if (launching !== null)
            return;
        problem = "";
        launching = {
            provider: row.provider,
            id: row.id
        };
        runRequested(row.provider, row.id, newWindow);
    }

    // Whether `row` is the one running.
    function isLaunching(row: var): bool {
        return launching !== null && launching.provider === row.provider && launching.id === row.id;
    }

    // novad's answer to runRequested: closes once it has run.
    function launched(error: var) {
        if (error) {
            problem = error;
            launching = null;
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
            const newWindow = (modifiers & Qt.ShiftModifier) !== 0;
            // The list still shows the previous query's matches.
            if (query.trim() !== "" && matchesFor !== query)
                enterPending = {
                    newWindow: newWindow
                };
            else if (rows.length > 0)
                launch(rows[selected], newWindow);
            return true;
        case Qt.Key_Escape:
            open = false;
            return true;
        // Tab stays in the field: Ctrl+Tab switches modes, Tab takes the
        // grey completion.
        case Qt.Key_Tab:
        case Qt.Key_Backtab:
            if ((modifiers & Qt.ControlModifier) !== 0)
                setMode(inFiles ? Protocol.PROVIDER_APPS : Protocol.PROVIDER_FILES);
            else if (completion !== "")
                field.text = rows[selected].file.completion;
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

    // Laid out at full size and scaled with the deck as it fades in. Closed,
    // it stays laid out under the center surface, so it takes no clicks then:
    // they belong to the panel open there (a closed row used to launch its
    // app). The search field takes the keyboard once it's enabled again.
    Item {
        enabled: launcher.open
        onEnabledChanged: {
            if (enabled)
                field.forceActiveFocus();
        }
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

                LineIcon {
                    id: searchIcon
                    anchors.verticalCenter: parent.verticalCenter
                    x: Tokens.launcherPad
                    size: Tokens.iconSizeMenu
                    path: Icons.SEARCH
                    color: Tokens.fgDim
                }

                TextInput {
                    id: field
                    objectName: "field"
                    anchors.verticalCenter: parent.verticalCenter
                    x: searchIcon.x + searchIcon.width + Tokens.launcherGap
                    width: (modes.visible ? modes.x : escHint.x) - x - Tokens.launcherGap
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeHeading
                    font.weight: Tokens.weightLabel
                    color: Tokens.fg
                    selectionColor: Tokens.primary
                    selectedTextColor: Tokens.onAccent
                    clip: true
                    maximumLength: Protocol.LAUNCHER_SEARCH_MAX_CHARS
                    // Typing reaches it only once the compositor has given
                    // the deck the keyboard, a moment after it opens.
                    onActiveFocusChanged: {
                        if (activeFocus && launcher.open)
                            console.info("nova: launcher has the keyboard");
                    }
                    cursorDelegate: Rectangle {
                        width: Tokens.caretWidth
                        color: Tokens.primary
                        visible: field.cursorVisible
                    }
                    onTextChanged: {
                        // A path is typed: the Files mode completes it.
                        if (LauncherLogic.typesPath(text) && !launcher.inFiles) {
                            launcher.setMode(Protocol.PROVIDER_FILES);
                            launcher.pathSwitched = launcher.inFiles;
                        } else if (text === "" && launcher.pathSwitched) {
                            launcher.setMode(Protocol.PROVIDER_APPS);
                        }
                        launcher.query = text;
                        launcher.enterPending = null;
                        launcher.selected = 0;
                        launcher.problem = "";
                        if (text.trim() !== "")
                            launcher.searchRequested(text, launcher.mode);
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
                        font.pixelSize: Tokens.fontSizeHeading
                        color: Tokens.fgDim
                        text: launcher.inFiles ? qsTr("Search files") : qsTr("Search apps")
                    }

                    // The rest of the selected file's path, which Tab takes.
                    Text {
                        objectName: "completion"
                        anchors.verticalCenter: parent.verticalCenter
                        x: field.contentWidth
                        width: Math.max(0, field.width - x)
                        visible: launcher.completion !== "" && field.cursorPosition === field.length && field.contentWidth < field.width
                        font: field.font
                        color: Tokens.fgDim
                        elide: Text.ElideRight
                        textFormat: Text.PlainText
                        text: Paths.shown(launcher.completion)
                    }
                }

                Segments {
                    id: modes
                    objectName: "modes"
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.right: escHint.left
                    anchors.rightMargin: Tokens.launcherGap
                    width: Tokens.launcherModesWidth
                    visible: launcher.filesMode
                    color: "transparent"
                    current: launcher.mode
                    options: [
                        {
                            key: Protocol.PROVIDER_APPS,
                            label: qsTr("Apps")
                        },
                        {
                            key: Protocol.PROVIDER_FILES,
                            label: qsTr("Files")
                        }
                    ]
                    onPicked: key => {
                        launcher.setMode(key);
                        field.forceActiveFocus();
                    }
                }

                Text {
                    id: escHint
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.right: parent.right
                    anchors.rightMargin: Tokens.launcherPad
                    font.family: Tokens.fontMono
                    font.pixelSize: Tokens.fontSizeSmall
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
                        onClicked: launcher.launch({
                            provider: Protocol.PROVIDER_APPS,
                            id: appId
                        }, false)
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
                text: (launcher.inFiles ? qsTr("Couldn't open it: %1") : qsTr("Couldn't start it: %1")).arg(launcher.problem)
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
                    color: launcher.launching !== null ? Tokens.alpha(Tokens.primary, Tokens.accentFillOpacity) : Tokens.selected
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
                    results: qsTr("Results"),
                    files: qsTr("Files")
                })[item.modelData.group] ?? ""
        }

        LauncherRow {
            objectName: "row"
            width: item.width
            app: item.modelData
            iconSource: launcher.iconSource(item.modelData.icon)
            selected: item.index === launcher.selected
            pinnable: item.modelData.provider === Protocol.PROVIDER_APPS
            pinned: pinnable && launcher.pinned.includes(item.modelData.id)
            launching: launcher.isLaunching(item.modelData)
            onHovered: at => launcher.pointed(item.index, at)
            onClicked: launcher.launch(item.modelData, false)
            onPinClicked: launcher.pinRequested(item.modelData.id, !pinned)
            onShowInFolderClicked: launcher.launch(item.modelData, true)
        }
    }

    component SectionLabel: Text {
        height: Tokens.launcherLabelHeight
        leftPadding: Tokens.rowInset
        verticalAlignment: Text.AlignVCenter
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeSmall
        font.capitalization: Font.AllUppercase
        color: Tokens.fgDim
    }
}
