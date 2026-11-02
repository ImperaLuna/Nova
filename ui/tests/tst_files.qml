import QtQuick
import QtTest
import "../files/paths.js" as Paths

// Nova Files' address bar parts and window title from a folder's path.
TestCase {
    name: "Files"

    function names(parts) {
        return parts.map(part => part.name);
    }

    function test_a_folder_in_home_starts_at_home() {
        const parts = Paths.crumbs("/home/ana/.local/share/Steam", "/home/ana", "Home");
        compare(names(parts), ["Home", ".local", "share", "Steam"]);
        compare(parts.map(part => part.path), ["/home/ana", "/home/ana/.local", "/home/ana/.local/share", "/home/ana/.local/share/Steam"]);
        compare(parts.map(part => part.hidden), [false, true, false, false]);
        compare(names(Paths.crumbs("/home/ana", "/home/ana", "Home")), ["Home"]);
    }

    function test_a_folder_outside_home_starts_at_the_root() {
        compare(names(Paths.crumbs("/home/anabel/Music", "/home/ana", "Home")), ["/", "home", "anabel", "Music"], "only home itself, not a name it starts");
        compare(names(Paths.crumbs("/mnt/usb", "", "Home")), ["/", "mnt", "usb"]);
        compare(names(Paths.crumbs("/", "/home/ana", "Home")), ["/"]);
    }

    function test_the_title_is_the_folder_name() {
        compare(Paths.folderName("/home/ana/Music"), "Music");
        compare(Paths.folderName("/"), "/");
    }
}
