.pragma library

// The region picker's sums, kept apart from QML so tests can reach them.

// CaptureIcon's names besides the capture modes and card actions.
var ICON_CLOSE = "close";
var ICON_CHECK = "check";
var ICON_DESKTOP_AUDIO = "desktop-audio";
var ICON_MIC = "mic";
var ICON_PAUSE = "pause";
var ICON_RESUME = "resume";
var ICON_STOP = "stop";
var ICON_VIDEO = "video";

// The rectangle between where a drag started and where it is now, in the
// window's logical pixels: {x, y, width, height}.
function region(fromX, fromY, toX, toY) {
    return {
        x: Math.min(fromX, toX),
        y: Math.min(fromY, toY),
        width: Math.abs(toX - fromX),
        height: Math.abs(toY - fromY)
    };
}

// Whether a drag's region is too small to be one (a click).
function isClick(rect, minDrag) {
    return rect.width < minDrag || rect.height < minDrag;
}

// A region in the window's logical pixels as the frame's pixels (save_capture's
// rect): the frame is the screen at its own scale, so `scale` is the frame's
// width over the window's. Rounded outward, so every pixel the outline
// touches is in, and kept on the frame.
function toPixels(rect, scale, frameWidth, frameHeight) {
    const left = Math.max(0, Math.floor(rect.x * scale));
    const top = Math.max(0, Math.floor(rect.y * scale));
    const right = Math.min(frameWidth, Math.ceil((rect.x + rect.width) * scale));
    const bottom = Math.min(frameHeight, Math.ceil((rect.y + rect.height) * scale));
    return {
        x: left,
        y: top,
        width: Math.max(0, right - left),
        height: Math.max(0, bottom - top)
    };
}

// The frame captured for the screen named `output`, from Daemon.capture, or
// null.
function frameFor(frames, output) {
    if (!frames)
        return null;
    for (let i = 0; i < frames.length; i++) {
        if (frames[i].output === output)
            return frames[i];
    }
    return null;
}

// "960 × 560", for the size badge and the card.
function size(width, height) {
    return `${width} × ${height}`;
}

// The folder a saved shot is in, as the card says it: its last two parts
// (Pictures/Screenshots).
function folderLabel(path) {
    const parts = path.split("/").filter(part => part.length > 0);
    return parts.slice(Math.max(0, parts.length - 3), parts.length - 1).join("/");
}

// A local file's URL, each part of the path escaped: a folder named
// `shots#1` stays part of the path.
function fileUrl(path) {
    return "file://" + path.split("/").map(encodeURIComponent).join("/");
}

// A region in a screen's logical pixels as a rectangle of the compositor's
// layout (start_recording's region): moved by the screen's place in it, and
// rounded inward to an even size, which video encoders need.
function toLayout(rect, screenX, screenY) {
    const left = Math.ceil(rect.x);
    const top = Math.ceil(rect.y);
    const width = Math.floor(rect.x + rect.width) - left;
    const height = Math.floor(rect.y + rect.height) - top;
    return {
        x: screenX + left,
        y: screenY + top,
        width: Math.max(0, width - width % 2),
        height: Math.max(0, height - height % 2)
    };
}

// A recording's time, "0:42" or "1:02:03", from milliseconds.
function clock(ms) {
    const total = Math.max(0, Math.floor(ms / 1000));
    const hours = Math.floor(total / 3600);
    const minutes = Math.floor(total / 60) % 60;
    const seconds = String(total % 60).padStart(2, "0");
    return hours > 0 ? `${hours}:${String(minutes).padStart(2, "0")}:${seconds}` : `${minutes}:${seconds}`;
}
