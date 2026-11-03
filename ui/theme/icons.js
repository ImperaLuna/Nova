.pragma library

// Nova's line icons as SVG paths on a 24-unit grid, for LineIcon: strokes,
// plus the solid parts some draw as its `fill`. Drawn in the style of the
// design mockups. Every icon in the UI comes from here, so each glyph is
// drawn one way.

var TUNE = "M4 7h9M17 7h3M4 17h3M11 17h9M15 5a2 2 0 1 1 0 4a2 2 0 1 1 0-4zM9 15a2 2 0 1 1 0 4a2 2 0 1 1 0-4z";
var POWER = "M12 3v8M6.3 7.2a8 8 0 1 0 11.4 0";
var CUP = "M4 9h12v5a5 5 0 0 1-5 5H9a5 5 0 0 1-5-5zM16 10h1.5a2.5 2.5 0 0 1 0 5H16M8 3.5c0 1.5 1 1.5 1 3M12 3.5c0 1.5 1 1.5 1 3";
var MIC = "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21";
var MIC_OFF = "M15 10V6a3 3 0 0 0-5.7-1.3M9 9v2a3 3 0 0 0 4.6 2.5M5.5 11a6.5 6.5 0 0 0 10.7 5M18.5 11a6.4 6.4 0 0 1-.6 2.7M12 17.5V21M3 3l18 18";
var CLOCK = "M12 3.5a8.5 8.5 0 1 1 0 17a8.5 8.5 0 1 1 0-17zM12 7.5V12l3 2";
var RIGHT = "M9 6l6 6-6 6";
var LEFT = "M15 6l-6 6 6 6";
var CHECK = "M5 12.5l4.5 4.5L19 7.5";
// The speaker's body and its waves apart, for the volume capsule's solid
// speaker with one, two or no waves.
var SPEAKER_BODY = "M4 9h4l5-4v14l-5-4H4z";
var WAVE = "M16.5 8.5a5 5 0 0 1 0 7";
var WAVES = WAVE + "M19 6a8.5 8.5 0 0 1 0 12";
var SPEAKER = SPEAKER_BODY + WAVES;
// A cross beside the speaker, for muted.
var MUTED = "M16.5 9.5l5 5M21.5 9.5l-5 5";
// Corner to corner, for something off or missing.
var STRIKE = "M3 3l18 18";
var HEADPHONES_BAND = "M4 15v-3a8 8 0 0 1 16 0v3";
var HEADPHONES = HEADPHONES_BAND + "M5 14h2.5v6H5a1 1 0 0 1-1-1v-4a1 1 0 0 1 1-1zM16.5 14H19a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1h-2.5z";
// The cups solid, under HEADPHONES_BAND.
var HEADPHONES_CUPS = "M4 15h3.5v6H5.5A1.5 1.5 0 0 1 4 19.5zM20 15h-3.5v6h2a1.5 1.5 0 0 0 1.5-1.5z";
var MONITOR = "M5 4h14a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM8 20h8M12 16v4";
// Two screens, one over the other's corner: every screen.
var MONITORS = "M3.5 4h9A1.5 1.5 0 0 1 14 5.5V9h-3.5A1.5 1.5 0 0 0 9 10.5V13H3.5A1.5 1.5 0 0 1 2 11.5v-6A1.5 1.5 0 0 1 3.5 4zM10.5 9h10a1.5 1.5 0 0 1 1.5 1.5v6a1.5 1.5 0 0 1-1.5 1.5h-10A1.5 1.5 0 0 1 9 16.5v-6A1.5 1.5 0 0 1 10.5 9zM13 21h5";
var BLUETOOTH = "M7 7l10 10-5 4V3l5 4L7 17";
var CLOSE = "M6 6l12 12M18 6L6 18";
var BELL = "M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0";
var BELL_OFF = "M6 16V11a6 6 0 0 1 9.5-4.9M18 11v5l1.5 2H8M10 20.5a2 2 0 0 0 4 0M3 3l18 18";
var GEAR = "M12 9a3 3 0 1 1 0 6a3 3 0 1 1 0-6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z";
// Three dots in a row, drawn solid (pass it as `fill` too).
var DOTS = "M6 10.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4zM12 10.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4zM18 10.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4z";
var SEARCH = "M10.5 4a6.5 6.5 0 1 1 0 13a6.5 6.5 0 0 1 0-13zM15.3 15.3L20 20";
// A tilde, centred on the grid: the central pill's special workspace.
var TILDE = "M5 13.5C7 10.5 9.5 10.5 12 12S17 13.5 19 10.5";
// A pushpin; filled with itself once pinned.
var PIN = "M9 4h6M10 4v5.5L7 13v1.5h10V13l-3-3.5V4M12 14.5V20";
// Nova's own mark, for a distribution without a logo: a four-point star.
var STAR = "M12 3c.6 4.6 2.4 6.4 7 7c-4.6.6-6.4 2.4-7 7c-.6-4.6-2.4-6.4-7-7c4.6-.6 6.4-2.4 7-7z";

// Capture: the region picker's corners, the screenshot card's actions and
// the recording controls.
var REGION = "M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3";
var OPEN = "M14 4h6v6M20 4l-9 9M18 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4";
var FOLDER = "M4 7a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z";
var TRASH = "M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3";
// A game's saves: a floppy disk (the Games place's Open save folder).
var SAVE = "M6 4h10l4 4v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM8 4v4h7V4M8 20v-6h8v6";
// Back where it was: an arrow coming round (the trash's Restore).
var RESTORE = "M4.5 12a7.5 7.5 0 1 0 2.2-5.3M4.5 4.5v4h4";
var CAMERA = "M4 8h3l2-3h6l2 3h3v11H4zM8.5 13a3.5 3.5 0 1 0 7 0a3.5 3.5 0 1 0-7 0";
var VIDEO = "M4 7a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2zM16 10l4-2.5v9L16 14";
var VIDEO_OFF = VIDEO + STRIKE;
// A file of no particular kind: a page with a folded corner; a document
// is one with lines of text.
var FILE = "M7 3h7l4 4v12a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM14 3v4h4";
var DOCUMENT = FILE + "M9 12h6M9 16h6";
var PAUSE = "M9 6v12M15 6v12";
// The now-playing controls: solid, so pass each as `fill` too.
var PAUSE_SOLID = "M7.7 5h1.6a1.2 1.2 0 0 1 1.2 1.2v11.6a1.2 1.2 0 0 1-1.2 1.2H7.7a1.2 1.2 0 0 1-1.2-1.2V6.2A1.2 1.2 0 0 1 7.7 5zM14.7 5h1.6a1.2 1.2 0 0 1 1.2 1.2v11.6a1.2 1.2 0 0 1-1.2 1.2h-1.6a1.2 1.2 0 0 1-1.2-1.2V6.2a1.2 1.2 0 0 1 1.2-1.2z";
var PLAY_SOLID = "M8 5l11 7-11 7z";
// Skipping: a solid triangle and a bar (pass SKIP_*_FILL as `fill`).
var SKIP_NEXT = "M6 5l9 7-9 7zM18 5v14";
var SKIP_NEXT_FILL = "M6 5l9 7-9 7z";
var SKIP_PREVIOUS = "M18 5l-9 7 9 7zM6 5v14";
var SKIP_PREVIOUS_FILL = "M18 5l-9 7 9 7z";
// A music note, for a track with no cover or a player with no icon.
var MUSIC = "M9 18V5l11-2v13M6.5 15.5a2.5 2.5 0 1 1 0 5a2.5 2.5 0 1 1 0-5zM17.5 13.5a2.5 2.5 0 1 1 0 5a2.5 2.5 0 1 1 0-5z";
// Up and out to the right: going to another window.
var GO_TO = "M7 17L17 7M9 7h8v8";
var PLAY = "M8 5.5v13l10-6.5z";
var STOP = "M7 7h10v10H7z";

// The power menu's actions (POWER is power off).
var LOCK = "M7.5 11h9a2.5 2.5 0 0 1 2.5 2.5v5a2.5 2.5 0 0 1-2.5 2.5h-9a2.5 2.5 0 0 1-2.5-2.5v-5a2.5 2.5 0 0 1 2.5-2.5zM8 11V8a4 4 0 0 1 8 0v3";
var MOON = "M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z";
var LOG_OUT = "M14 4h4a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-4M10 8l-4 4 4 4M6 12h10";
var REBOOT = "M4 12a8 8 0 1 0 2.4-5.7M4 4v4h4";

// Wireless devices in quick settings, by kind, and how they connect.
var MOUSE = "M12 3a6 6 0 0 1 6 6v6a6 6 0 0 1-12 0V9a6 6 0 0 1 6-6zM12 7v4";
var KEYBOARD = "M4.5 6h15a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-15a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2zM6 10h.01M9.5 10h.01M13 10h.01M16.5 10h.01M7 14h10";
var GAMEPAD = "M7 8h10a4 4 0 0 1 4 4.5l-.6 3.6a2 2 0 0 1-3.5 1L15 15H9l-1.9 2.1a2 2 0 0 1-3.5-1L3 12.5A4 4 0 0 1 7 8zM7.5 11v3M6 12.5h3M15.5 12h.01M17.5 13.5h.01";
// Anything else with a battery: a small device with a button.
var DEVICE = "M8 3h8a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM11 18h2";
// A 2.4 GHz receiver: two arcs over a dot.
var DONGLE = "M5 12a7 7 0 0 1 14 0M8 12a4 4 0 0 1 8 0M12 13h.01";
// A cable, for a device plugged in.
var PLUG = "M9 3v5M15 3v5M7 8h10v3a5 5 0 0 1-10 0zM12 16v5";
// Charging: solid, so pass it as `fill` too.
var BOLT = "M13 2L4 14h7l-1 8 9-12h-7z";

// Network: Wi-Fi's arcs, outer to inner, over a solid dot (pass WIFI_DOT as
// `fill`), with WifiIcon fading the arcs a weak signal doesn't reach; Wi-Fi
// struck out; an Ethernet port, plugged and struck out.
var WIFI_ARC_OUTER = "M2 8.5a15 15 0 0 1 20 0";
var WIFI_ARC_MIDDLE = "M5 12a10.5 10.5 0 0 1 14 0";
var WIFI_ARC_INNER = "M8.5 15.5a5.5 5.5 0 0 1 7 0";
var WIFI = WIFI_ARC_OUTER + WIFI_ARC_MIDDLE + WIFI_ARC_INNER;
var WIFI_DOT = "M12 17.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4z";
var WIFI_OFF = "M2 8.5a15 15 0 0 1 4.6-2.9M10.5 4.6A15 15 0 0 1 22 8.5M5 12a10.5 10.5 0 0 1 3.2-2M15 9.6a10.5 10.5 0 0 1 4 2.4" + WIFI_ARC_INNER + STRIKE;
var PORT = "M7.5 4h9A1.5 1.5 0 0 1 18 5.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 6 12.5v-7A1.5 1.5 0 0 1 7.5 4zM9 14v3h6v-3M12 17v3";
var WIRED = PORT + "M9 7v2M12 7v2M15 7v2";
var WIRED_OFF = PORT + STRIKE;
// A password field's show and hide.
var EYE = "M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12zM12 9a3 3 0 1 1 0 6a3 3 0 1 1 0-6z";
var EYE_OFF = "M10 5.7A9.6 9.6 0 0 1 12 5.5c6 0 9.5 6.5 9.5 6.5a17 17 0 0 1-2.6 3.4M6.6 6.6C3.9 8.3 2.5 12 2.5 12s3.5 6.5 9.5 6.5a9.3 9.3 0 0 0 5.4-1.7M9.9 9.9a3 3 0 0 0 4.2 4.2" + STRIKE;
// Nova Files: places in the sidebar, moving up a folder, and the command
// row's actions. Back and forward are LEFT and RIGHT.
var HOME = "M4 11l8-6.5 8 6.5v8.5h-5.5v-5h-5v5H4z";
var DESKTOP = "M5 4.5h14a1.5 1.5 0 0 1 1.5 1.5v9a1.5 1.5 0 0 1-1.5 1.5H5a1.5 1.5 0 0 1-1.5-1.5V6A1.5 1.5 0 0 1 5 4.5zM9 20h6M12 16.5V20";
var DOWNLOAD = "M12 4v11M7.5 10.5L12 15l4.5-4.5M5 19.5h14";
var PICTURE = "M5.5 5h13a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2zM6 17l4.5-5 3 3.5 2-2 3 3.5M15.5 7.5a1.5 1.5 0 1 1 0 3a1.5 1.5 0 1 1 0-3z";
var UP = "M12 19V6M6.5 11.5L12 6l5.5 5.5";
var CHEVRON_DOWN = "M7 9.5l5 5 5-5";
var PLUS = "M12 5v14M5 12h14";
var CUT = "M7 14.5a2.5 2.5 0 1 1 0 5a2.5 2.5 0 1 1 0-5zM17 14.5a2.5 2.5 0 1 1 0 5a2.5 2.5 0 1 1 0-5zM8.8 15.2L17 4M15.2 15.2L7 4";
var COPY = "M10.5 8.5h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-7a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2zM15.5 8.5V6a1.5 1.5 0 0 0-1.5-1.5H6A1.5 1.5 0 0 0 4.5 6v8A1.5 1.5 0 0 0 6 15.5h2.5";
var PASTE = "M9 4.5h6v3H9zM15 6h2.5v14h-11V6H9";
var RENAME = "M4 20h4L19 9l-4-4L4 16zM13 7l4 4";
var SORT = "M7 5v14M4 16l3 3 3-3M17 19V5M14 8l3-3 3 3";
var VIEW = "M4 6h16M4 12h16M4 18h16";
// Nova Files' drives: a disk inside the machine, a USB stick, and Eject.
var DISK = "M5.5 7h13a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2zM7 12h.01M10 12h7";
var USB = "M8 9.5h8V20H8zM9.5 4h5v5.5h-5zM11 6.5h.01M13 6.5h.01";
var EJECT = "M12 6l6 7H6zM6 17.5h12";
