pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "details.js" as Details
import "listing.js" as Listing
import "paths.js" as Paths

// Nova Files' details pane, on the right of the file list (Show details,
// like COSMIC's): the item at `path` with its big icon, name and type, its
// facts (items or size, created, modified, accessed), then the app that
// opens it, and who may read, write and run it. It reads through `service`
// (FilesService, given by the window) whenever `path` changes or
// `reload()` asks, keeping only the latest answer. The cross closes it.
Rectangle {
    id: pane

    property var service: null
    // The item it describes, an absolute path; empty for none.
    property string path: ""
    // What novad said about it (files_details), or null.
    property var details: null
    property int _asked: 0

    signal closeRequested

    function reload() {
        const asked = ++_asked;
        if (path === "" || !service) {
            details = null;
            return;
        }
        service.details(path, found => {
            if (asked === pane._asked)
                pane.details = found;
        });
    }

    readonly property var row: details ? Listing.row(details.entry) : null

    color: Tokens.glass
    onPathChanged: reload()
    onServiceChanged: reload()

    OpenWithModel {
        id: openWith
        service: pane.service
        path: pane.row && !pane.row.dir ? pane.path : ""
    }

    // Its edge against the list.
    Rectangle {
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: Tokens.dividerWidth
        color: Tokens.glassRim
    }

    Column {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Tokens.filesDetailsPad
        spacing: Tokens.filesDetailsPad
        visible: pane.row !== null

        Column {
            width: parent.width
            spacing: Tokens.labelNudge

            LineIcon {
                anchors.horizontalCenter: parent.horizontalCenter
                size: Tokens.filesDetailsIconSize
                path: pane.row ? Listing.icon(pane.row) : ""
                color: pane.row?.dir ? Tokens.filesFolder : Tokens.fgDim
            }
            Text {
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeHeading
                font.weight: Tokens.weightTitle
                color: Tokens.fg
                elide: Text.ElideMiddle
                textFormat: Text.PlainText
                text: pane.row ? Paths.shown(pane.row.name) : ""
            }
            Text {
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                color: Tokens.fgDim
                text: pane.row ? Listing.kind(pane.row) : ""
            }
        }

        Column {
            width: parent.width
            spacing: Tokens.labelNudge * 2

            Repeater {
                model: pane.details ? Details.facts(pane.details) : []

                Item {
                    id: fact

                    required property var modelData

                    width: parent.width
                    height: value.implicitHeight

                    Text {
                        anchors.left: parent.left
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        color: Tokens.fgDim
                        text: fact.modelData.label
                    }
                    Text {
                        id: value
                        anchors.right: parent.right
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        color: Tokens.fg
                        text: fact.modelData.value
                    }
                }
            }
        }

        Column {
            width: parent.width
            spacing: Tokens.dividerWidth

            Line {
                visible: openWith.apps.length > 0
                label: qsTr("Open with")
                value: openWith.apps[0]?.name ?? ""
            }
            Repeater {
                model: pane.details ? Details.access(pane.details) : []

                Line {
                    required property var modelData

                    label: modelData.who
                    role: modelData.role
                    value: modelData.can
                }
            }
        }
    }

    component Line: Rectangle {
        id: line

        property string label
        property string role
        property string value

        width: parent?.width ?? 0
        height: Tokens.filesDetailsRowHeight
        color: Tokens.surfaceRaised

        Column {
            anchors.left: parent.left
            anchors.leftMargin: Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width / 2 - Tokens.rowInset

            Text {
                width: parent.width
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                color: Tokens.fg
                elide: Text.ElideRight
                textFormat: Text.PlainText
                text: line.label
            }
            Text {
                visible: line.role !== ""
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeMicro
                color: Tokens.filesFaint
                text: line.role
            }
        }
        Text {
            anchors.right: parent.right
            anchors.rightMargin: Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width / 2 - Tokens.rowInset
            horizontalAlignment: Text.AlignRight
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeSmall
            color: Tokens.fgDim
            wrapMode: Text.Wrap
            maximumLineCount: 2
            elide: Text.ElideRight
            textFormat: Text.PlainText
            text: line.value
        }
    }

    Rectangle {
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Tokens.gap
        width: Tokens.filesButtonSize
        height: Tokens.filesButtonSize
        radius: Tokens.radiusControl
        color: closeHover.hovered ? Tokens.hover : "transparent"

        LineIcon {
            anchors.centerIn: parent
            path: Icons.CLOSE
            color: Tokens.fgDim
        }
        HoverHandler {
            id: closeHover
        }
        TapHandler {
            onTapped: pane.closeRequested()
        }
    }
}
