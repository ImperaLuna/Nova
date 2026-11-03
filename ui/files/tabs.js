.pragma library

// The arithmetic of a Nova Files window's tabs, as plain functions so
// ui/tests can check them: which tab Ctrl+Tab lands on, which one shows
// when the shown one closes, how wide each is, and where a dragged tab
// lands or when it comes out as a window of its own.

// The index `steps` on from `at` among `count` tabs, round from the last
// to the first and back.
function cycled(at, count, steps) {
    return ((at + steps) % count + count) % count;
}

// The tab that shows after the one at `at` (the shown one) closes: the
// next one, or the one before when it was the last. Indexes are into the
// tabs before it closes.
function neighbour(at, count) {
    return at + 1 < count ? at + 1 : at - 1;
}

// Each tab's width when `count` share `room`, `gap` apart: as wide as
// `widest` while they fit, narrower down to `narrowest`.
function width(room, count, gap, narrowest, widest) {
    if (count <= 0)
        return widest;
    const share = Math.floor((room - gap * (count - 1)) / count);
    return Math.max(narrowest, Math.min(widest, share));
}

// A tab let go `moved` px up (negative) or down from where it was picked
// up comes out as a window when that's at least `distance` off the row,
// and only when another tab stays behind.
function tornOut(moved, distance, count) {
    return count > 1 && Math.abs(moved) >= distance;
}

// Where a tab at `at` lands when let go `moved` px along the row, each
// tab taking `step` px: the nearest place, among `count`.
function dropped(at, moved, step, count) {
    return Math.max(0, Math.min(count - 1, at + Math.round(moved / step)));
}

// `tabs` with the one at `from` moved to `to`.
function reordered(tabs, from, to) {
    const rest = tabs.filter((_, index) => index !== from);
    rest.splice(to, 0, tabs[from]);
    return rest;
}

// The row's scroll (`scroll`, px) that brings a tab at `x`, `size` wide,
// into a view `view` wide, moving it as little as it can.
function revealed(scroll, x, size, view) {
    if (x < scroll)
        return x;
    if (x + size > scroll + view)
        return x + size - view;
    return scroll;
}
