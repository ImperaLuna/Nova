.pragma library

// Spans and times of day as Nova writes them: "3d 2h", "2h 14m", "45m";
// "5m" ago; a recording's "1:02:03"; "09:05". Callers round to whole
// minutes their own way (a countdown up, an uptime down).

var SECOND_MS = 1000;
var MINUTE_SECONDS = 60;
var MINUTE_MS = MINUTE_SECONDS * SECOND_MS;
var HOUR_MINUTES = 60;
var HOUR_SECONDS = HOUR_MINUTES * MINUTE_SECONDS;
var DAY_MINUTES = 24 * HOUR_MINUTES;
var DAY_SECONDS = DAY_MINUTES * MINUTE_SECONDS;

// `minutes` (a whole number) as days and hours, or hours and minutes,
// leaving out a zero second part.
function minutes(minutes) {
    var days = Math.floor(minutes / DAY_MINUTES);
    var hours = Math.floor(minutes % DAY_MINUTES / HOUR_MINUTES);
    var rest = minutes % HOUR_MINUTES;
    if (days > 0)
        return hours > 0 ? days + "d " + hours + "h" : days + "d";
    if (hours > 0)
        return rest > 0 ? hours + "h " + rest + "m" : hours + "h";
    return rest + "m";
}

// "up 3h 12m" for `seconds` since boot, whole minutes rounded down.
function uptime(seconds) {
    return qsTr("up %1").arg(minutes(Math.max(0, Math.floor(seconds / MINUTE_SECONDS))));
}

// How long ago `time` was at `now` (both seconds since the epoch), in its
// biggest whole unit: "now", "5m", "2h", "3d".
function ago(time, now) {
    var seconds = Math.max(0, now - time);
    if (seconds < MINUTE_SECONDS)
        return qsTr("now");
    if (seconds < HOUR_SECONDS)
        return qsTr("%1m").arg(Math.floor(seconds / MINUTE_SECONDS));
    if (seconds < DAY_SECONDS)
        return qsTr("%1h").arg(Math.floor(seconds / HOUR_SECONDS));
    return qsTr("%1d").arg(Math.floor(seconds / DAY_SECONDS));
}

// A running time like a stopwatch's, "0:42" or "1:02:03", from
// milliseconds.
function clock(ms) {
    var total = Math.max(0, Math.floor(ms / SECOND_MS));
    var hours = Math.floor(total / HOUR_SECONDS);
    var minutes = Math.floor(total / MINUTE_SECONDS) % HOUR_MINUTES;
    var seconds = twoDigits(total % MINUTE_SECONDS);
    return hours > 0 ? hours + ":" + twoDigits(minutes) + ":" + seconds : minutes + ":" + seconds;
}

// A time of day's hours or minutes, "09".
function twoDigits(number) {
    return number < 10 ? "0" + number : String(number);
}
