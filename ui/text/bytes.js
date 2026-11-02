.pragma library

// Sizes as Nova writes them, in steps of 1024: short "9.4G" for tight
// spots, "2.4 GB" for lists and dialogs. One decimal under 10.

var UNITS = ["B", "K", "M", "G", "T"];
var STEP = 1024;
// Past this a size moves up a unit, so it never reads "1012K".
var MOST_DIGITS = 1000;

// A size in bytes, short: "9.4G", "780M", "42K".
function shortBytes(bytes) {
    var value = bytes;
    var unit = 0;
    while (value >= MOST_DIGITS && unit < UNITS.length - 1) {
        value /= STEP;
        unit++;
    }
    var digits = value < 10 && unit > 0 ? 1 : 0;
    return value.toFixed(digits) + UNITS[unit];
}

// A size in bytes for a list: "2.4 GB", "310 MB".
function bytes(bytes) {
    var short = shortBytes(bytes);
    var unit = short.slice(-1);
    return short.slice(0, -1) + " " + (unit === "B" ? "B" : unit + "B");
}
