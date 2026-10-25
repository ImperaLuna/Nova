.pragma library

// Spans of time as Nova writes them: "3d 2h", "2h 14m", "45m". Callers
// round to whole minutes their own way (a countdown up, an uptime down).

var MINUTE_MS = 60 * 1000;
var HOUR_MINUTES = 60;
var DAY_MINUTES = 24 * HOUR_MINUTES;

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
