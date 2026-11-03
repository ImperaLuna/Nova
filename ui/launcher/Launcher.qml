pragma ComponentBehavior: Bound

import QtQuick
import "../stage"
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "launcher.js" as LauncherLogic

// The launcher: a glass deck near the top of the middle of the screen that
// fades in where it stands (the central pill stays). A search field, the
// pinned apps as tiles, and a list: the recent apps (every app before the
// first launch), or what a search's providers find (apps first: the
// launcher.providers slot order). Up and Down move through the
// list, Enter launches (focusing an open app's window where it is;
// Shift+Enter opens another), Esc or a click outside closes. Opened by
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
    // The recent apps the list shows: novad's latest, held while a launch
    // runs and while the deck is closed, so a launched app's move to the top
    // shows the next time it opens, not as the deck fades out.
    property var shownRecent: []
    onRecentChanged: {
        if (open && launching === null)
            shownRecent = recent;
    }
    readonly property var rows: LauncherLogic.rows(query, matches, shownRecent, apps, known)
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
        shownRecent = recent;
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

    // novad's matches for `forQuery`; dropped when something else has been
    // typed since.
    function showMatches(forQuery: string, found: var) {
        if (forQuery !== query)
            return;
        matches = found;
        matchesFor = forQuery;
        const pending = enterPending;
        enterPending = null;
        if (pending && rows.length > 0)
            launch(rows[0], pending.newWindow);
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

    // Whether the list's selected row is the one running: a pinned tile's
    // launch leaves the list as it is.
    readonly property bool launchingSelected: launching !== null && selected < rows.length && isLaunching(rows[selected])

    // novad's answer to runRequested: closes once it has run.
    function launched(error: var) {
        if (error) {
            problem = error;
            launching = null;
            shownRecent = recent;
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
                    anchors.verticalCenter: parent.verticalCenter
                    x: searchIcon.x + searchIcon.width + Tokens.launcherGap
                    width: escHint.x - x - Tokens.launcherGap
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
                        launcher.query = text;
                        launcher.enterPending = null;
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
                        font.pixelSize: Tokens.fontSizeHeading
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
                        launching: launcher.isLaunching({
                            provider: Protocol.PROVIDER_APPS,
                            id: appId
                        })
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
                    color: launcher.launchingSelected ? Tokens.alpha(Tokens.primary, Tokens.accentFillOpacity) : Tokens.selected
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
            objectName: "row"
            width: item.width
            app: item.modelData
            iconSource: launcher.iconSource(item.modelData.icon)
            selected: item.index === launcher.selected
            pinnable: item.modelData.provider === Protocol.PROVIDER_APPS
            pinned: pinnable && launcher.pinned.includes(item.modelData.id)
            launching: selected && launcher.launchingSelected
            onHovered: at => launcher.pointed(item.index, at)
            onClicked: {
                if (launcher.launching !== null)
                    return;
                launcher.selected = item.index;
                launcher.launch(item.modelData, false);
            }
            onPinClicked: launcher.pinRequested(item.modelData.id, !pinned)
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
