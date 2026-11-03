.pragma library

// Sizes in bytes as people read them, shared by every list that shows one
// (quick settings' resources and network pages, Nova Files' Size column),
// and drive sizes as their labels give them.

// A size in bytes, short: "9.4G", "780M", "42K". One decimal under 10.
function shortBytes(bytes) {
    var units = ["B", "K", "M", "G", "T"];
    var value = bytes;
    var unit = 0;
    while (value >= 1000 && unit < units.length - 1) {
        value /= 1024;
        unit++;
    }
    var digits = value < 10 && unit > 0 ? 1 : 0;
    return value.toFixed(digits) + units[unit];
}

// A size in bytes for a list: "2.4 GB", "310 MB".
function bytes(bytes) {
    var short = shortBytes(bytes);
    var unit = short.slice(-1);
    return short.slice(0, -1) + " " + (unit === "B" ? "B" : unit + "B");
}

// A drive's size or free space as the drive's label gives it, in
// thousands: "212 GB", "1 TB", "9.5 GB". One decimal under 10, dropped
// when it's 0.
function storage(bytes) {
    var units = ["B", "KB", "MB", "GB", "TB", "PB"];
    var value = bytes;
    var unit = 0;
    while (value >= 999.5 && unit < units.length - 1) {
        value /= 1000;
        unit++;
    }
    var shown = value < 9.95 && unit > 0 ? value.toFixed(1).replace(/\.0$/, "") : value.toFixed(0);
    return shown + " " + units[unit];
}
