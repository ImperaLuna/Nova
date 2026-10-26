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
var BLUETOOTH = "M7 7l10 10-5 4V3l5 4L7 17";
var CLOSE = "M6 6l12 12M18 6L6 18";
var BELL = "M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0";
var BELL_OFF = "M6 16V11a6 6 0 0 1 9.5-4.9M18 11v5l1.5 2H8M10 20.5a2 2 0 0 0 4 0M3 3l18 18";
var GEAR = "M12 9a3 3 0 1 1 0 6a3 3 0 1 1 0-6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z";
// Three dots in a row, drawn solid (pass it as `fill` too).
var DOTS = "M6 10.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4zM12 10.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4zM18 10.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4z";
var SEARCH = "M10.5 4a6.5 6.5 0 1 1 0 13a6.5 6.5 0 0 1 0-13zM15.3 15.3L20 20";
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
var CAMERA = "M4 8h3l2-3h6l2 3h3v11H4zM8.5 13a3.5 3.5 0 1 0 7 0a3.5 3.5 0 1 0-7 0";
var VIDEO = "M4 7a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2zM16 10l4-2.5v9L16 14";
var PAUSE = "M9 6v12M15 6v12";
var PLAY = "M8 5.5v13l10-6.5z";
var STOP = "M7 7h10v10H7z";

// The power menu's actions (POWER is power off).
var LOCK = "M7.5 11h9a2.5 2.5 0 0 1 2.5 2.5v5a2.5 2.5 0 0 1-2.5 2.5h-9a2.5 2.5 0 0 1-2.5-2.5v-5a2.5 2.5 0 0 1 2.5-2.5zM8 11V8a4 4 0 0 1 8 0v3";
var MOON = "M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z";
var LOG_OUT = "M14 4h4a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-4M10 8l-4 4 4 4M6 12h10";
var REBOOT = "M4 12a8 8 0 1 0 2.4-5.7M4 4v4h4";
