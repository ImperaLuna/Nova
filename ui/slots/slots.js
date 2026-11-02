.pragma library

// Slots: the places a feature's UI goes, from the `[[slot]]` entries of its
// manifest (features/<id>/feature.toml), which novad sends with each
// feature's state. A host (the bar's rows, the right pill's buttons, the
// stage's panels, quick settings' sections and pages) shows the views of its slots, sorted by `order`, for the
// features that are ready. The views themselves are in views.js.

// Every host, as nova_features::HOSTS names them (a Rust test checks the
// two lists match).
var HOST_BAR_LEFT = "bar.left";
var HOST_BAR_CENTER = "bar.center";
var HOST_BAR_RIGHT = "bar.right";
var HOST_RIGHT_PILL_BUTTONS = "right-pill.buttons";
var HOST_STAGE_PANELS = "stage.panels";
var HOST_QUICK_SETTINGS_TILES = "quick-settings.tiles";
var HOST_QUICK_SETTINGS_SLIDERS = "quick-settings.sliders";
var HOST_QUICK_SETTINGS_CARDS = "quick-settings.cards";
var HOST_QUICK_SETTINGS_FILL = "quick-settings.fill";
var HOST_QUICK_SETTINGS_FOOT = "quick-settings.foot";
var HOST_QUICK_SETTINGS_PAGES = "quick-settings.pages";

// The slots in `host` of the ready features in `features` (the features
// event's list), as {feature, view, order}: lowest order first, ties by
// feature id.
function forHost(features, host) {
    var found = [];
    for (var i = 0; i < features.length; i++) {
        var feature = features[i];
        if (feature.status !== "ready")
            continue;
        var slots = feature.slots || [];
        for (var j = 0; j < slots.length; j++) {
            if (slots[j].host === host)
                found.push({ feature: feature.id, view: slots[j].view, order: slots[j].order || 0 });
        }
    }
    found.sort(function (a, b) {
        if (a.order !== b.order)
            return a.order - b.order;
        return a.feature < b.feature ? -1 : a.feature > b.feature ? 1 : 0;
    });
    return found;
}

// Whether two lists from forHost show the same views in the same order, so
// a host can keep the views it has when a features event changes nothing
// for it.
function same(a, b) {
    if (a.length !== b.length)
        return false;
    for (var i = 0; i < a.length; i++) {
        if (a[i].view !== b[i].view)
            return false;
    }
    return true;
}
