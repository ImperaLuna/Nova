.pragma library

// Nova's line icons as SVG paths on a 24-unit grid, for LineIcon: strokes,
// plus FILLS for the few with a solid part. Drawn in the style of the
// design mockups.

var TUNE = "M4 7h9M17 7h3M4 17h3M11 17h9M15 5a2 2 0 1 1 0 4a2 2 0 1 1 0-4zM9 15a2 2 0 1 1 0 4a2 2 0 1 1 0-4z";
var POWER = "M12 3v8M6.3 7.2a8 8 0 1 0 11.4 0";
var CUP = "M4 9h12v5a5 5 0 0 1-5 5H9a5 5 0 0 1-5-5zM16 10h1.5a2.5 2.5 0 0 1 0 5H16M8 3.5c0 1.5 1 1.5 1 3M12 3.5c0 1.5 1 1.5 1 3";
var MIC = "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21";
var MIC_OFF = "M15 10V6a3 3 0 0 0-5.7-1.3M9 9v2a3 3 0 0 0 4.6 2.5M5.5 11a6.5 6.5 0 0 0 10.7 5M18.5 11a6.4 6.4 0 0 1-.6 2.7M12 17.5V21M3 3l18 18";
var CLOCK = "M12 3.5a8.5 8.5 0 1 1 0 17a8.5 8.5 0 1 1 0-17zM12 7.5V12l3 2";
var RIGHT = "M9 6l6 6-6 6";
var LEFT = "M15 6l-6 6 6 6";
var CHECK = "M5 12.5l4.5 4.5L19 7.5";
var SPEAKER = "M4 9h4l5-4v14l-5-4H4zM16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12";
var HEADPHONES = "M4 15v-3a8 8 0 0 1 16 0v3M5 14h2.5v6H5a1 1 0 0 1-1-1v-4a1 1 0 0 1 1-1zM16.5 14H19a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1h-2.5z";
var MONITOR = "M5 4h14a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM8 20h8M12 16v4";
var BLUETOOTH = "M7 7l10 10-5 4V3l5 4L7 17";
var CLOSE = "M6 6l12 12M18 6L6 18";
var BELL = "M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0";
var BELL_OFF = "M6 16V11a6 6 0 0 1 9.5-4.9M18 11v5l1.5 2H8M10 20.5a2 2 0 0 0 4 0M3 3l18 18";
var GEAR = "M12 9a3 3 0 1 1 0 6a3 3 0 1 1 0-6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z";
// Three dots in a row, drawn solid (pass it as `fill` too).
var DOTS = "M6 10.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4zM12 10.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4zM18 10.8a1.2 1.2 0 1 1 0 2.4a1.2 1.2 0 1 1 0-2.4z";
var SEARCH = "M11 4.5a6.5 6.5 0 1 1 0 13a6.5 6.5 0 1 1 0-13zM16 16l4.5 4.5";
// Nova's own mark, for a distribution without a logo: a four-point star.
var STAR = "M12 3c.6 4.6 2.4 6.4 7 7c-4.6.6-6.4 2.4-7 7c-.6-4.6-2.4-6.4-7-7c4.6-.6 6.4-2.4 7-7z";
