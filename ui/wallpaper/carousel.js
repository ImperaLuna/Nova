.pragma library

// The wallpaper carousel's pure logic: it loops, so going past either end
// comes back round to the other. Its position is a real number that runs on
// without wrapping (each step adds or takes one), so a step across the ends
// moves like any other; which image that is, and where each card sits
// around it, wrap.

// `index` as one of `count` images (0 to count - 1), however far it went.
function wrap(index, count) {
    return ((index % count) + count) % count;
}

// How many cards image `index` sits right (positive) or left (negative) of
// the one at `position`, the short way round: never more than half the
// images either way.
function offset(index, position, count) {
    let away = (index - position) % count;
    if (away > count / 2)
        away -= count;
    else if (away <= -count / 2)
        away += count;
    return away;
}

// How much of a card `away` from the middle shows (0 to 1): `sides` cards
// show either side, and each image once. One fades out before it'd pass
// halfway round, where it jumps to the other side, so none flies across;
// in an even loop the one at rest exactly halfway shows on the right
// (where offset puts it), so two images show both. It jumps there as the
// deck comes to rest, so the deck fades it in.
function shown(away, count, sides) {
    const near = (sides + 0.5 - Math.abs(away)) * 2;
    const resting = Number.isInteger(away) && away === count / 2;
    const beforeHalfway = resting ? 1 : (count / 2 - Math.abs(away)) * 2;
    return Math.max(0, Math.min(1, near, beforeHalfway));
}

// Where to start: on the image shown now when it's in the folder, else the
// first.
function startIndex(images, current) {
    return Math.max(0, images.indexOf(current));
}

// A path's file name.
function fileName(path) {
    return path.slice(path.lastIndexOf("/") + 1);
}
