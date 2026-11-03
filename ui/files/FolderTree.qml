pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "drops.js" as Drops
import "navigation.js" as Navigation
import "paths.js" as Paths

// The sidebar's folder tree, as novad lays it out for the window
// (files_tree): open down to the window's folder, which is marked and
// kept in view, with hidden folders dimmer. A row's chevron opens or
// closes it; a click elsewhere on the row goes there. Rows that come slide in one
// after another, the others move to make room. A folder below the top
// drags out (`dragRequested`); while a drag is over the tree, the folder a
// drop would go into (`dropFolder`) washes in the accent, and `placeAt`
// says which it is, opening when the pointer rests on it.
ListView {
    id: tree

    required property Navigator navigator
    // The folder a drag over the tree would drop into, or "".
    property string dropFolder: ""

    signal dragRequested(string path)
    // While an update adds rows, the rows created slide in, staggered by
    // how many came before them; rows created by scrolling just show.
    property bool arriving: false
    property int arrivals: 0

    // The place under (x, y) in the tree's own coordinates, for a
    // DropTarget: a closed folder with folders in it opens by resting on it.
    function placeAt(x: real, y: real): var {
        const index = indexAt(x + contentX, y + contentY);
        if (index < 0)
            return Drops.NOWHERE;
        const row = rows.get(index);
        return Drops.place(row.path, !row.open && !row.leaf);
    }

    function update() {
        arrivals = 0;
        arriving = true;
        Navigation.sync(rows, navigator.treeRows);
        arriving = false;
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
        color: tree.dropFolder === path ? Tokens.filesDropTarget : current ? Tokens.selected : hover.hovered ? Tokens.hover : "transparent"

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
            if (!tree.arriving) {
                opacity = 1;
                rise.y = 0;
                return;
            }
            wait.duration = tree.arrivals++ * Tokens.filesTreeStagger;
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
                text: item.home ? qsTr("Home") : Paths.shown(item.name)
            }
        }

        HoverHandler {
            id: hover
        }
        // Home (or the root) stays put; folders under it drag.
        DragHandler {
            target: null
            enabled: item.depth > 0
            // The tree doesn't scroll by dragging; the wheel scrolls it.
            grabPermissions: PointerHandler.CanTakeOverFromAnything
            onActiveChanged: {
                if (active)
                    tree.dragRequested(item.path);
            }
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
    }
}
