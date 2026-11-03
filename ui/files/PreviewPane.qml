pragma ComponentBehavior: Bound

import QtQuick
import "service"
import "../theme"
import "../daemon/protocol.js" as Protocol
import "listing.js" as Listing
import "paths.js" as Paths
import "preview.js" as Preview

// Nova Files' preview pane (Alt+P), at the file list's right, on the
// list's own surface past a thin rule. It previews the one selected file
// once the selection has rested (Tokens.filesPreviewDelay), asking novad
// (files_preview) off the UI thread: a picture (an image itself, a PDF's
// first page, a video's still) decoded asynchronously and fitted to the
// pane, or the first lines of text, scrollable from the top. A selected
// file that changes on disk is previewed anew. A folder, a file with
// nothing to show and a picture Qt can't read show their icon and name;
// several selected show how many; nothing selected, nothing. A slow answer
// gets a spinner; a late answer for a file passed by is dropped
// (Preview.face says which face shows).
Item {
    id: pane

    required property string folder
    required property FolderModel model
    // How solid the list's surface is (0..1), as FileList's.
    property real solid: 0

    // How many are selected, and the one previewed (Preview.subject): a
    // plain copy of its row, or null.
    property int count: 0
    property var row: null
    // novad's answer for `row`: null until it comes, false when it failed.
    property var preview: null
    // Counts asks, so only the answer to the latest one shows.
    property int asking: 0
    // The answer is slow in coming: the spinner shows.
    property bool slow: false
    readonly property string face: Preview.face(count, row, preview, picture.status === Image.Error)

    // Follows the selection: a different row starts over, after the rest.
    function follow() {
        count = model.selectedCount;
        const picked = count === 1 ? model.selectedRows()[0] : null;
        const next = Preview.subject(count, picked ? Listing.copy(picked) : null);
        // The same file as it was, unless asking about it failed.
        const same = next !== null && row !== null && preview !== false && next.name === row.name && next.modified === row.modified && next.size === row.size;
        if (same)
            return;
        row = next;
        preview = null;
        slow = false;
        asking++;
        if (Preview.asks(row)) {
            rest.restart();
            slowTimer.restart();
        } else {
            rest.stop();
            slowTimer.stop();
        }
    }

    function ask() {
        const asked = ++asking;
        FilesService.preview(Paths.child(folder, row.name), answer => {
            if (asked === pane.asking) {
                slowTimer.stop();
                pane.preview = answer;
            }
        });
    }

    Component.onCompleted: follow()

    Connections {
        target: pane.model
        function onRevisionChanged() {
            pane.follow();
        }
    }
    // A row changes in place when its file does, or moves (a remove and an
    // insert) when the order is by what changed.
    Connections {
        target: pane.model.rows
        function onDataChanged() {
            pane.follow();
        }
        function onRowsInserted() {
            pane.follow();
        }
    }
    Timer {
        id: rest
        interval: Tokens.filesPreviewDelay
        onTriggered: pane.ask()
    }
    Timer {
        id: slowTimer
        interval: Tokens.filesPreviewDelay + Tokens.filesPreviewSlow
        onTriggered: pane.slow = true
    }

    Rectangle {
        anchors.fill: parent
        color: Tokens.mix(Tokens.glass, Tokens.filesSolidList, pane.solid)
    }
    Rectangle {
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.topMargin: Tokens.filesRuleInset
        anchors.bottomMargin: Tokens.filesRuleInset
        width: Tokens.dividerWidth
        color: Tokens.filesRule
    }

    Item {
        id: room
        anchors.fill: parent
        anchors.margins: Tokens.filesPreviewPad

        Image {
            id: picture
            anchors.fill: parent
            source: pane.preview?.kind === Protocol.PREVIEW_IMAGE ? Paths.fileUrl(pane.preview.path) : ""
            asynchronous: true
            cache: false
            autoTransform: true
            fillMode: Image.PreserveAspectFit
            sourceSize: Qt.size(Protocol.FILES_PREVIEW_PICTURE_PX, Protocol.FILES_PREVIEW_PICTURE_PX)
            opacity: pane.face === Preview.IMAGE && status === Image.Ready ? 1 : 0

            Behavior on opacity {
                NumberAnimation {
                    duration: Tokens.durationFast
                }
            }
        }

        Flickable {
            id: scroller
            anchors.fill: parent
            visible: pane.face === Preview.TEXT
            clip: true
            contentHeight: lines.implicitHeight
            boundsBehavior: Flickable.StopAtBounds

            Text {
                id: lines
                width: parent.width
                font.family: Tokens.fontMono
                font.pixelSize: Tokens.fontSizeSmall
                color: Tokens.fgDim
                wrapMode: Text.Wrap
                textFormat: Text.PlainText
                text: pane.preview?.kind === Protocol.PREVIEW_TEXT ? pane.preview.text : ""
                onTextChanged: scroller.contentY = 0
            }
        }

        Column {
            anchors.centerIn: parent
            width: parent.width
            visible: pane.face === Preview.ITEM
            spacing: Tokens.gap

            LineIcon {
                anchors.horizontalCenter: parent.horizontalCenter
                path: pane.row ? Listing.icon(pane.row) : ""
                size: Tokens.filesPreviewIconSize
                color: pane.row?.dir ? Tokens.filesFolder : Tokens.fgDim
            }
            Text {
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                color: Tokens.fg
                wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                maximumLineCount: Tokens.filesPreviewNameLines
                elide: Text.ElideRight
                textFormat: Text.PlainText
                text: pane.row ? Paths.shown(pane.row.name) : ""
            }
        }

        Text {
            anchors.centerIn: parent
            visible: pane.face === Preview.COUNT
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeHeading
            color: Tokens.filesFaint
            text: Listing.items(pane.count)
        }

        Spinner {
            anchors.centerIn: parent
            visible: pane.face === Preview.WAITING && pane.slow
            spinning: visible
            color: Tokens.filesFaint
        }
    }
}
