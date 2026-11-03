pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "arrival.js" as Arrival
import "navigation.js" as Navigation
import "paths.js" as Paths

// The sidebar's folder tree, as novad lays it out for the window
// (files_tree): open down to the window's folder, which is marked and
// kept in view, with hidden folders dimmer. The top is the home folder,
// by its own name (Home is the Home page). A row's chevron opens or
// closes it; a click elsewhere on the row goes there, a middle click asks
// for it in a new tab (`tabRequested`), and a right-click asks for its
// menu (pinning it to Quick access). Rows that come slide in one after
// another, the others move to make room.
ListView {
    id: tree

    required property Navigator navigator

    signal tabRequested(string path)
    // While an update adds rows, the rows created slide in one after
    // another once it's done, by the file list's wave rule (Arrival); rows
    // created by scrolling just show.
    property bool arriving: false
    property var arrivals: []
    readonly property var pace: ({
            stagger: Tokens.filesTreeStagger,
            wave: Tokens.filesRowWave,
            most: Tokens.filesRowWaveMost
        })

    // The menu of the folder `path` was asked for at `at` (in the window).
    signal menuRequested(string path, point at)

    function update() {
        arrivals = [];
        arriving = true;
        Navigation.sync(rows, navigator.treeRows);
        arriving = false;
        // A row the update made and removed again is gone already.
        const came = arrivals.filter(item => item?.arriveAfter);
        arrivals = [];
        came.forEach((item, order) => item.arriveAfter(Arrival.wait(order, came.length, came.length, pace)));
        for (let i = 0; i < rows.count; i++) {
            if (rows.get(i).path === navigator.folder) {
                positionViewAtIndex(i, ListView.Contain);
                break;
            }
        }
    }

    clip: true
    boundsBehavior: Flickable.StopAtBounds
    spacing: Tokens.dividerWidth
    model: ListModel {
        id: rows
    }

    Connections {
        target: tree.navigator
        function onTreeRowsChanged() {
            tree.update();
        }
    }
    Component.onCompleted: update()
    // Another tab came to the front.
    onNavigatorChanged: update()

    displaced: Transition {
        NumberAnimation {
            property: "y"
            duration: Tokens.durationPanel
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }

    delegate: Rectangle {
        id: item

        required property string name
        required property string path
        required property int depth
        required property bool open
        required property bool leaf
        required property bool hidden
        readonly property bool current: path === tree.navigator.folder
        // The tree's top is Home, unless the window is outside it.
        readonly property bool home: depth === 0 && path !== "/"

        width: ListView.view.width
        height: Tokens.filesPlaceHeight
        radius: Tokens.menuRowRadius
        color: current ? Tokens.selected : hover.hovered ? Tokens.hover : "transparent"

        Behavior on color {
            ColorAnimation {
                duration: Tokens.durationFast
            }
        }

        // Slides in from just above, after the rows that came before it.
        opacity: 0
        transform: Translate {
            id: rise
            y: -Tokens.filesTreeRise
        }
        Component.onCompleted: {
            if (tree.arriving)
                tree.arrivals.push(item);
            else
                arriveAfter(-1);
        }
        // Eases in after `ms`, or just shows for -1.
        function arriveAfter(ms: int) {
            if (ms < 0) {
                opacity = 1;
                rise.y = 0;
                return;
            }
            wait.duration = ms;
            arrive.start();
        }
        SequentialAnimation {
            id: arrive
            PauseAnimation {
                id: wait
            }
            ParallelAnimation {
                NumberAnimation {
                    target: item
                    property: "opacity"
                    to: 1
                    duration: Tokens.durationPanel
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Tokens.easing
                }
                NumberAnimation {
                    target: rise
                    property: "y"
                    to: 0
                    duration: Tokens.durationPanel
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Tokens.easing
                }
            }
        }

        Row {
            id: content
            anchors.left: parent.left
            anchors.leftMargin: Tokens.gap + item.depth * Tokens.filesTreeIndent
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Tokens.filesTreeGap

            LineIcon {
                id: chevron
                anchors.verticalCenter: parent.verticalCenter
                size: Tokens.filesTreeChevron
                color: Tokens.filesFaint
                path: Icons.RIGHT
                // Kept in place without subfolders, so names line up.
                opacity: item.leaf ? 0 : 1
                rotation: item.open ? 90 : 0

                Behavior on rotation {
                    NumberAnimation {
                        duration: Tokens.durationPanel
                        easing.type: Easing.BezierSpline
                        easing.bezierCurve: Tokens.easing
                    }
                }
            }
            LineIcon {
                anchors.verticalCenter: parent.verticalCenter
                color: item.current ? Tokens.primary : item.hidden ? Tokens.filesFaint : Tokens.fgDim
                path: item.home ? Icons.HOME : Icons.FOLDER
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width - x
                leftPadding: Tokens.filesTreeGap
                elide: Text.ElideRight
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: item.current ? Tokens.weightLabel : Tokens.weightBody
                color: item.hidden ? Tokens.filesFaint : Tokens.fg
                textFormat: Text.PlainText
                text: Paths.shown(item.name)
            }
        }

        HoverHandler {
            id: hover
        }
        // One handler for the row, so a click on the chevron doesn't also
        // go to the folder: up to the chevron's far edge it opens or closes.
        TapHandler {
            onTapped: point => {
                if (!item.leaf && point.position.x < content.x + chevron.x + chevron.width)
                    tree.navigator.toggle(item.path, item.open);
                else
                    tree.navigator.visit(item.path, null);
            }
        }
        TapHandler {
            acceptedButtons: Qt.MiddleButton
            onTapped: tree.tabRequested(item.path)
        }
        TapHandler {
            acceptedButtons: Qt.RightButton
            onTapped: point => tree.menuRequested(item.path, item.mapToItem(null, point.position))
        }
    }
}
