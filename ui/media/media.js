.pragma library
.import "../text/duration.js" as Duration
.import "../daemon/protocol.js" as Protocol

// What the now-playing pill and panel show from novad's `media` event
// ({players, shown}). novad picks the player to show and checks what it
// sends; this only finds it, moves its position on while it plays, and
// turns numbers into text.

var MICROS_PER_MS = 1000;

// The player the bar shows, or null.
function shown(media) {
    if (!media || !media.shown)
        return null;
    return find(media, media.shown);
}

// The player `id`, or null.
function find(media, id) {
    var players = media ? media.players : [];
    for (var i = 0; i < players.length; i++) {
        if (players[i].id === id)
            return players[i];
    }
    return null;
}

function playing(player) {
    return player !== null && player.status === Protocol.PLAYBACK_PLAYING;
}

// Where `player` is in its track at `nowMs` (ms since 1970), in
// microseconds: where novad last read it, moved on at its rate while it
// plays, kept within the track.
function position(player, nowMs) {
    if (!player)
        return 0;
    var at = player.position_us;
    if (playing(player))
        at += Math.max(0, nowMs - player.position_at_ms) * MICROS_PER_MS * player.rate;
    if (player.length_us > 0)
        at = Math.min(at, player.length_us);
    return Math.max(0, at);
}

// How far through its track `player` is at `nowMs`, 0..1; 0 when its
// length is unknown.
function progress(player, nowMs) {
    if (!player || player.length_us <= 0)
        return 0;
    return position(player, nowMs) / player.length_us;
}

// A position or length in microseconds as "3:58", or "1:02:03".
function time(micros) {
    return Duration.clock(micros / MICROS_PER_MS);
}

// The position at `fraction` (0..1, kept within) of the way through the
// track, in microseconds, for media_seek.
function seekTarget(player, fraction) {
    return Math.round(Math.min(1, Math.max(0, fraction)) * player.length_us);
}

// What the player's app is called: its installed app's name, else what
// the player calls itself.
function appName(player) {
    if (!player)
        return "";
    return player.app ? player.app.name : player.identity;
}

// Whether the pill shows the artist: browsers put the channel or site
// there, so their videos show only the title.
function pillArtist(player) {
    return player !== null && !player.browser && player.artist !== "";
}

// The players the panel lists as chips to switch between: every one not
// stopped, once there are two or more.
function chips(media) {
    var players = media ? media.players.filter(function (player) {
        return player.status !== Protocol.PLAYBACK_STOPPED;
    }) : [];
    return players.length >= 2 ? players : [];
}

// The four equalizer bars' heights, each between `low` and `high`
// (whole pixels), from `random` (Math.random, or a fixed one in tests).
function equalizer(count, low, high, random) {
    var heights = [];
    for (var i = 0; i < count; i++)
        heights.push(low + Math.round(random() * (high - low)));
    return heights;
}

// How long one scroll through a title `over` px too long takes, in ms,
// at `speed` px per second.
function marqueeDuration(over, speed) {
    return Math.max(0, over) / speed * 1000;
}
