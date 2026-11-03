.pragma library

// How new rows arrive in Nova Files' lists (the file list, the sidebar's
// tree): each eases in after the ones before it, but the arrival is only
// decoration and never a wait. The whole wave fits in `wave` ms however
// many rows take part, so the step between rows shrinks as they grow;
// with more than `most` rows coming in, they all come in as one piece.

// How long the row `order` places into the wave waits before it eases in,
// in ms: `places` is how many places the wave has on screen, `coming` how
// many rows came in, and `pace` holds the list's {stagger, wave, most}
// (rows step `stagger` ms apart at most). -1 for a row that takes no part
// (order -1) and just shows.
function wait(order, places, coming, pace) {
    if (order < 0)
        return -1;
    if (coming > pace.most || places < 2)
        return 0;
    return Math.round(order * Math.min(pace.stagger, pace.wave / (places - 1)));
}

// A file list row's place in the wave rippling out from row `from`, with
// `page` rows on screen, or -1 for a row that takes no part: no wave
// running (`from` -1), or a row past the screen, which shows at once
// (rows below the fold, and pages that come in later).
function order(index, from, page) {
    if (from < 0)
        return -1;
    const distance = Math.abs(index - from);
    return distance <= page ? distance : -1;
}
