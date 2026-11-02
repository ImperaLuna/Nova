.pragma library
.import "../daemon/protocol.js" as Protocol

// Slots: the places a feature's UI goes, from the `[[slot]]` entries of its
// manifest (features/<id>/feature.toml), which novad sends with each
// feature's state. A host (the bar's rows, the right pill's buttons, the
// stage's panels, quick settings' sections and pages) shows the views of
// its slots, sorted by `order`, for the features that are ready. The views
// themselves are in Views.qml.

// Every host, as nova_features::HOSTS names them (a Rust test checks the
// two lists match).
var HOST_BAR_LEFT = "bar.left";
var HOST_BAR_CENTER = "bar.center";
var HOST_BAR_RIGHT = "bar.right";
var HOST_RIGHT_PILL_BUTTONS = "right-pill.buttons";
var HOST_CENTRAL_PILL_PARTS = "central-pill.parts";
var HOST_STAGE_PANELS = "stage.panels";
var HOST_SHELL_OVERLAYS = "shell.overlays";
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
    var ready = Protocol.readyFeatures(features);
    var found = [];
    for (var i = 0; i < features.length; i++) {
        var feature = features[i];
        if (ready.indexOf(feature.id) === -1)
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

// Makes `model` (a ListModel of {feature, view, order}) list `wanted` (from
// forHost) in its order, moving, adding and removing only what changed, so
// a host keeps the views (and their open panels) that stay.
function sync(model, wanted) {
    for (var i = model.count - 1; i >= 0; i--) {
        var view = model.get(i).view;
        if (!wanted.some(function (slot) {
                return slot.view === view;
            }))
            model.remove(i);
    }
    for (var j = 0; j < wanted.length; j++) {
        var at = -1;
        for (var k = j; k < model.count; k++) {
            if (model.get(k).view === wanted[j].view) {
                at = k;
                break;
            }
        }
        if (at === -1)
            model.insert(j, wanted[j]);
        else if (at !== j)
            model.move(at, j, 1);
        else
            model.setProperty(j, "order", wanted[j].order);
    }
}
