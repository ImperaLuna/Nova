.pragma library
.import "../text/duration.js" as Duration

// What the AI usage pill and panel show for one provider of novad's
// `ai_usage` event. novad does the work (sources, resets); this only turns
// its numbers into text.

// From this percent a limit turns the theme's primary color. One rule, no
// further tiers.
var WARN_PERCENT = 80;

var NAMES = {
    claude: "Claude",
    codex: "Codex"
};

var WINDOW_NAMES = {
    session: "5-hour session",
    weekly: "Weekly"
};

var DAY_MS = Duration.DAY_MINUTES * Duration.MINUTE_MS;
var WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

function isWarn(percent) {
    return percent >= WARN_PERCENT;
}

// "2h 14m", "45m", "3d 2h". Rounded up, so it never reads 0m before the reset.
function duration(ms) {
    return Duration.minutes(Math.max(1, Math.ceil(ms / Duration.MINUTE_MS)));
}

function pad(number) {
    return number < 10 ? "0" + number : String(number);
}

// "2h 14m left", or "" once the window has reset.
function leftText(resetsAt, now) {
    if (!resetsAt)
        return "";
    var ms = new Date(resetsAt).getTime() - now.getTime();
    return ms > 0 ? duration(ms) + " left" : "";
}

// "Resets in 2h 14m" within a day, else "Resets Mon 04:00" in local time.
function resetText(resetsAt, now) {
    if (!resetsAt)
        return "";
    var at = new Date(resetsAt);
    var ms = at.getTime() - now.getTime();
    if (ms <= 0)
        return "";
    if (ms < DAY_MS)
        return "Resets in " + duration(ms);
    return "Resets " + WEEKDAYS[at.getDay()] + " " + pad(at.getHours()) + ":" + pad(at.getMinutes());
}

// Everything the pill and panel need for `provider` (one entry of the
// event's `providers`) at `now`.
function summarize(provider, now) {
    var limits = provider && provider.state === "ready" ? provider.limits || [] : [];
    var rows = limits.map(function (limit) {
        return {
            name: WINDOW_NAMES[limit.window] || limit.window,
            percent: limit.percent,
            warn: isWarn(limit.percent),
            left: leftText(limit.resets_at, now),
            reset: resetText(limit.resets_at, now)
        };
    });
    var main = rows.length > 0 ? rows[0] : null;
    var state = provider ? provider.state : "loading";
    return {
        id: provider ? provider.provider : "",
        name: provider ? NAMES[provider.provider] || provider.provider : "",
        plan: provider && provider.plan ? provider.plan : "",
        state: state,
        // The session limit when there is one, else the only window.
        main: main,
        second: rows.length > 1 ? rows[1] : null,
        hero: main ? String(main.percent) : state === "loading" ? "…" : "–",
        warn: main ? main.warn : false,
        message: main ? "" : state === "loading" ? "Checking your plan limits…" : provider.hint || ""
    };
}

// The provider entry the bar shows.
function shown(usage) {
    if (!usage)
        return null;
    for (var i = 0; i < usage.providers.length; i++) {
        if (usage.providers[i].provider === usage.shown)
            return usage.providers[i];
    }
    return usage.providers.length > 0 ? usage.providers[0] : null;
}

// The other providers, in novad's order.
function others(usage) {
    return usage ? usage.providers.filter(function (p) {
        return p.provider !== usage.shown;
    }) : [];
}
