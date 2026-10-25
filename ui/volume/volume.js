.pragma library

// The volume capsule's pure logic: what a scroll or click sets the volume
// to, which icon and number it shows, when a change from PipeWire is worth
// showing, the springs that move it and where the capsule is drawn on each
// frame. No QML types, so ui/tests can check it all.

// Wheel delta of one notch (Qt's angleDelta, in eighths of a degree).
var NOTCH = 120;

// The volume `direction` (1 or -1) steps of `step` from `volume`, on the
// step grid. Nova's own controls stop at 100%: up leaves a volume another
// app set above it alone, down steps from 100%.
function stepped(volume, direction, step) {
    if (direction > 0 && volume >= 1)
        return volume;
    var from = Math.min(volume, 1);
    var next = Math.round((from + direction * step) / step) * step;
    return atFraction(next);
}

// The volume for a point `fraction` of the way up the capsule, to the
// percent.
function atFraction(fraction) {
    return Math.round(clamp(fraction, 0, 1) * 100) / 100;
}

// How full the capsule is for this volume: full above 100%, empty without
// an output.
function level(volume, available) {
    return available ? clamp(volume, 0, 1) : 0;
}

function percent(volume) {
    return Math.round(volume * 100);
}

// "none" (no output to control), "headphones" or "speakers", from the
// default sink's PipeWire properties (pw-cli i <id>). Headphones and
// headsets say so in their form factor or icon name.
function outputKind(available, properties) {
    if (!available)
        return "none";
    var props = properties || {};
    var hints = [props["device.form-factor"], props["device.icon-name"], props["node.icon-name"]];
    for (var i = 0; i < hints.length; i++) {
        if (/head(phone|set)/.test(hints[i] || ""))
            return "headphones";
    }
    return "speakers";
}

// Which glyph VolumeIcon draws: "none", "muted", "headphones", or a speaker
// with "off" (0%), "low" or "high" waves.
function icon(kind, muted, volume) {
    if (kind === "none")
        return "none";
    if (muted)
        return "muted";
    if (kind === "headphones")
        return "headphones";
    if (volume <= 0)
        return "off";
    return volume < 0.5 ? "low" : "high";
}

// Whether PipeWire's new state (`now`: {id, volume, muted}, the default
// sink's) is a change to show. The first state seen is where things start,
// not a change, so starting Nova shows nothing; another sink is a new
// output, which shows.
function isChange(before, now) {
    if (!before)
        return false;
    if (before.id !== now.id)
        return true;
    return before.volume !== now.volume || before.muted !== now.muted;
}

// One step of a damped spring from `x` (velocity `v`) towards `target`:
// {x, v, a}. `stiffness` and `damping` (ratio, 1 is critical) are tokens.
function spring(x, v, target, stiffness, damping, dt) {
    var c = 2 * Math.sqrt(stiffness) * damping;
    var a = -stiffness * (x - target) - c * v;
    v += a * dt;
    return {
        x: x + v * dt,
        v: v,
        a: a
    };
}

// Where the capsule is drawn in a window `width` x `height` against the
// right edge, `open` (0 in, 1 out, overshooting) and moving at `speed`
// (open per second): {x, y, w, h, radius}. It grows from half its height
// as it comes out, and stretches thinner and taller while it moves fast.
// `size` holds the tokens: width, height, gap, stretch, overshoot.
function box(open, speed, width, height, size) {
    var out = Math.max(0, open);
    var stretch = 1 + size.stretch * Math.min(1, Math.abs(speed) / 6);
    var h = size.height * (0.5 + 0.5 * Math.min(out, 1 + size.overshoot)) * stretch;
    var w = size.width / stretch;
    return {
        x: width - out * (size.width + size.gap) + (size.width - w) / 2,
        y: (height - h) / 2,
        w: w,
        h: h,
        radius: w / 2
    };
}

// The tallest box() gets, for the window's size.
function tallest(size) {
    return size.height * (1 + size.overshoot / 2) * (1 + size.stretch);
}

// The liquid's surface, from the top of a capsule `height` tall with glass
// `wall` thick, at `level`. Empty, it's the bottom.
function surfaceY(level, height, wall) {
    if (level <= 0)
        return height;
    return wall + (1 - level) * (height - 2 * wall);
}

function clamp(x, lo, hi) {
    return Math.max(lo, Math.min(hi, x));
}
