import QtQuick
import "../theme"
import "../text/files.js" as Files

// A file's thumbnail in Nova Files' Icons view: asked of novad as the tile
// is made (the view only makes tiles on screen and near it), cancelled as
// it scrolls away, and asked again when the file changes. Fitted into the
// box without cropping, with a hairline around the picture, and faded in
// once loaded; `shown` stays false until then, so the tile keeps its type
// icon for a file without one.
Item {
    id: thumbnail

    // The file's absolute path, or "" for none (a folder).
    required property string path
    // When the file last changed: a new time asks again.
    required property real modified
    // What thumbnails are asked of: FilesService (thumbnail and
    // cancelThumbnail), or null for none.
    required property var thumbnails

    readonly property bool shown: image.status === Image.Ready

    // The PNG novad answered with for `path` as it is, or "".
    property string _file: ""
    // The ask still waiting: {path, of (the service), done}, or null.
    property var _waiting: null
    // Changes only count once it's made: its first values come in one by
    // one, and the first ask has them all.
    property bool _made: false

    function _ask() {
        _cancel();
        _file = "";
        if (path === "" || !thumbnails)
            return;
        const asked = {
            path: path,
            of: thumbnails
        };
        asked.done = file => {
            if (thumbnail._waiting !== asked)
                return;
            thumbnail._waiting = null;
            thumbnail._file = file;
        };
        _waiting = asked;
        thumbnails.thumbnail(path, modified, asked.done);
    }

    function _cancel() {
        const asked = _waiting;
        _waiting = null;
        asked?.of.cancelThumbnail(asked.path, asked.done);
    }

    onPathChanged: {
        if (_made)
            _ask();
    }
    onModifiedChanged: {
        if (_made)
            _ask();
    }
    onThumbnailsChanged: {
        if (_made)
            _ask();
    }
    Component.onCompleted: {
        _made = true;
        _ask();
    }
    Component.onDestruction: _cancel()

    Image {
        id: image
        anchors.centerIn: parent
        width: Math.min(thumbnail.width, implicitWidth * thumbnail.height / Math.max(implicitHeight, 1))
        height: Math.min(thumbnail.height, implicitHeight * thumbnail.width / Math.max(implicitWidth, 1))
        source: thumbnail._file !== "" ? Files.fileUrl(thumbnail._file) : ""
        sourceSize: Qt.size(thumbnail.width * Screen.devicePixelRatio, thumbnail.height * Screen.devicePixelRatio)
        fillMode: Image.PreserveAspectFit
        asynchronous: true
        // A file made again keeps its cache name: read it anew.
        cache: false
        smooth: true
        opacity: thumbnail.shown ? 1 : 0

        Behavior on opacity {
            NumberAnimation {
                duration: Tokens.durationFade
            }
        }

        Rectangle {
            anchors.fill: parent
            color: "transparent"
            border.width: Tokens.dividerWidth
            border.color: Tokens.glassRim
        }
    }
}
