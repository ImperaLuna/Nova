.pragma library
.import "../daemon/protocol.js" as Protocol
.import "../text/bytes.js" as Bytes
.import "../theme/icons.js" as Icons
.import "listing.js" as Listing
.import "games.js" as Games
.import "paths.js" as Paths
.import "trash.js" as Trash

// Nova Files' Home page and the places around it, kept apart so ui/tests
// can check them: the page's name and address, which places aren't
// folders (the Home page, the games and the trash), how a sidebar place or a Quick
// access tile reads, a drive's free space, and a recent file as a row.

// The Home page's name, in the sidebar, the address bar and the window
// title.
function name() {
    return qsTr("Home");
}

// Whether a window at `folder` shows the Home page.
function isHome(folder) {
    return folder === Protocol.FILES_HOME_PLACE;
}

// Whether a window at `folder` shows a page rather than a folder: the Home
// page, the games or the trash, which have nothing above them.
function isPage(folder) {
    return isHome(folder) || Games.isGames(folder) || Trash.isTrash(folder);
}

// The page at `folder`'s name, in the address bar and the window title.
function pageName(folder) {
    return isHome(folder) ? name() : Games.isGames(folder) ? Games.name() : Trash.name();
}

// The address bar's one part while a window shows the page at `folder`.
function crumbs(folder) {
    return [{
            name: pageName(folder),
            path: folder,
            hidden: false
        }];
}

// Whether a place ({kind, path, name} from files_places) is a folder
// pinned to Quick access.
function isPinned(place) {
    return place.kind !== Protocol.PLACE_HOME && place.kind !== Protocol.PLACE_GAMES && place.kind !== Protocol.PLACE_TRASH;
}

// How a place reads: {name, icon}. The Home page, the games and the trash
// have their own names; a pinned folder has the name novad gives (its
// bookmark's label or its own), and its kind's icon.
function look(place) {
    switch (place.kind) {
    case Protocol.PLACE_HOME:
        return {
            name: name(),
            icon: Icons.HOME
        };
    case Protocol.PLACE_GAMES:
        return {
            name: Games.name(),
            icon: Icons.GAMEPAD
        };
    case Protocol.PLACE_TRASH:
        return {
            name: Trash.name(),
            icon: Icons.TRASH
        };
    }
    return {
        name: Paths.shown(place.name),
        icon: ICONS[place.kind] ?? Icons.FOLDER
    };
}

// The user's folders' icons, by place kind.
var ICONS = {
    [Protocol.PLACE_DESKTOP]: Icons.DESKTOP,
    [Protocol.PLACE_DOCUMENTS]: Icons.DOCUMENT,
    [Protocol.PLACE_DOWNLOADS]: Icons.DOWNLOAD,
    [Protocol.PLACE_PICTURES]: Icons.PICTURE,
    [Protocol.PLACE_MUSIC]: Icons.MUSIC,
    [Protocol.PLACE_VIDEOS]: Icons.VIDEO
};

// The folders pinned to Quick access among `places`, in order.
function pinned(places) {
    return places.filter(place => isPinned(place));
}

// Whether the folder `path` is pinned among `places`.
function pinnedIn(places, path) {
    return places.some(place => isPinned(place) && place.path === path);
}

// Whether novad said how full a drive is: only while it's mounted.
function measured(drive) {
    return drive.mount !== undefined && drive.used !== undefined && drive.size > 0;
}

// A drive's free space under its bar, as Explorer reads it: "120 GB free of
// 500 GB"; empty while it isn't measured.
function free(drive) {
    if (!measured(drive))
        return "";
    return qsTr("%1 free of %2").arg(Bytes.bytes(Math.max(0, drive.size - drive.used))).arg(Bytes.bytes(drive.size));
}

// How full a drive is, 0 to 1; 0 while it isn't measured.
function fill(drive) {
    if (!measured(drive))
        return 0;
    return Math.min(1, drive.used / drive.size);
}

// What a Recent row holds: a recent file ({folder, used_ms, entry}) as the
// file list's rows hold an entry, plus the folder it's in and when it was
// used (-1 when the list doesn't say).
function recentRow(file) {
    const row = Listing.row(file.entry);
    row.folder = file.folder;
    row.used = file.used_ms ?? -1;
    return row;
}
