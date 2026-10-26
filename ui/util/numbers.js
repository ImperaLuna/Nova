.pragma library

// Small number helpers shared across the UI.

// `x` kept within `lo`..`hi`.
function clamp(x, lo, hi) {
    return Math.max(lo, Math.min(hi, x));
}
