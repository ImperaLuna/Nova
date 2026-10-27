.pragma library

// Plain logic for the now-playing pill and panel: which player the pill
// shows, the progress line's position and the times it reads. novad decides
// the shown player and sends each player's position with the time it was
// read; the UI only extrapolates while the panel is open (see NowPlaying).

var SECONDS_PER_MINUTE = 60;
var MINUTES_PER_HOUR = 60;
var MICROSECONDS_PER_MILLISECOND = 1000;
var MICROSECONDS_PER_SECOND = 1000000;
var MILLISECONDS_PER_SECOND = 1000;

// The player `media.shown` names, or null when none is shown.
function shownPlayer(media) {
    if (!media || !media.shown || !media.players)
        return null;
    for (const player of media.players) {
        if (player.id === media.shown)
            return player;
    }
    return null;
}

// Where a playing player is now, in microseconds: what it said at its read
// time, moved on at its rate, never past the end of the track. A paused or
// stopped one stays where it was.
function positionAt(player, nowMs) {
    if (player.status !== "playing")
        return player.position_us;
    const elapsedUs = (nowMs - player.position_at_ms) * MICROSECONDS_PER_MILLISECOND * player.rate;
    const position = player.position_us + elapsedUs;
    return Math.min(Math.max(position, 0), player.length_us);
}

// A time in microseconds as "m:ss", or "h:mm:ss" from an hour on.
function formatTime(microseconds) {
    const totalSeconds = Math.floor(Math.max(microseconds, 0) / MICROSECONDS_PER_SECOND);
    const seconds = totalSeconds % SECONDS_PER_MINUTE;
    const totalMinutes = Math.floor(totalSeconds / SECONDS_PER_MINUTE);
    const minutes = totalMinutes % MINUTES_PER_HOUR;
    const hours = Math.floor(totalMinutes / MINUTES_PER_HOUR);
    const pad = number => (number < 10 ? "0" : "") + number;
    if (hours > 0)
        return `${hours}:${pad(minutes)}:${pad(seconds)}`;
    return `${minutes}:${pad(seconds)}`;
}

// The fraction of a track that's played, 0 to 1; 0 for a track of no length.
function fraction(position, length) {
    return length > 0 ? Math.min(Math.max(position / length, 0), 1) : 0;
}

// The name for the header and the "Go to" row: the app's, else the player's.
function appName(player) {
    return player.app ? player.app.name : player.identity;
}

// The artist shown in the pill. A browser's "artist" is its channel or site,
// so it's left out there.
function pillArtist(player) {
    return player.browser ? "" : player.artist;
}

// The "Go to" row's text: "Go to Spotify · workspace 3", or just "Go to
// Spotify" when novad didn't find the window's workspace.
function goToLabel(player, workspace) {
    const name = appName(player);
    return workspace ? `Go to ${name} · workspace ${workspace}` : `Go to ${name}`;
}

// Whether a player's status makes the pill show its equalizer moving.
function isPlaying(player) {
    return !!player && player.status === "playing";
}
