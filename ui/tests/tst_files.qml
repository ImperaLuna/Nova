import QtQuick
import QtTest
import "../files/paths.js" as Paths
import "../daemon/protocol.js" as Protocol

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

    // Names that aren't UTF-8 show a replacement character per run of raw
    // bytes, and keep their raw form in paths.
    function test_raw_bytes_show_as_one_replacement_character() {
        const eAcute = String.fromCodePoint(Protocol.RAW_BYTE_BASE + 0xE9);
        const grave = String.fromCodePoint(Protocol.RAW_BYTE_BASE + 0xE8);
        compare(Paths.shown(`caf${eAcute}.md`), "caf\uFFFD.md");
        compare(Paths.shown(`${eAcute}${grave}t${eAcute}`), "\uFFFDt\uFFFD");
        compare(Paths.shown("café ü"), "café ü");
        const parts = Paths.crumbs(`/mnt/M${eAcute}sik`, "", "Home");
        compare(names(parts), ["/", "mnt", "M\uFFFDsik"]);
        compare(parts[2].path, `/mnt/M${eAcute}sik`);
        compare(Paths.folderName(`/mnt/M${eAcute}sik`), "M\uFFFDsik");
    }

    function test_the_title_is_the_folder_name() {
        compare(Paths.folderName("/home/ana/Music"), "Music");
        compare(Paths.folderName("/"), "/");
    }
}
