pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland

// A stage surface's input mask and blur region (Stage, PanelSurface): one
// ShapeRegion per item in `inputShapes` and `glassShapes`. The window sets
// `mask` as its mask; blur is asked for once there's glass, since asking at
// all logs a warning on compositors without blur, which a screen with no
// glass shouldn't do. Real blur needs ext-background-effect-v1 (niri
// 26.04+, Hyprland 0.56+); elsewhere the tint stands alone.
QtObject {
    id: stageRegions

    // The window whose blur this is.
    required property QsWindow window
    required property list<Item> inputShapes
    required property list<Item> glassShapes
    // Quickshell rebuilds a region when its item's own position or size
    // changes, not an ancestor's: following this (an otherwise empty
    // region's y) rebuilds both when items move with an ancestor, like the
    // bar's pills sliding with their shelf (IMP-118).
    property real offset: 0

    readonly property Region mask: Region {
        y: stageRegions.offset
        regions: inputRegions.instances
    }
    readonly property Region blur: Region {
        y: stageRegions.offset
        regions: blurRegions.instances
    }

    readonly property Variants inputRegions: Variants {
        id: inputRegions
        model: stageRegions.inputShapes
        ShapeRegion {}
    }
    readonly property Variants blurRegions: Variants {
        id: blurRegions
        model: stageRegions.glassShapes
        ShapeRegion {}
    }

    readonly property bool hasGlass: glassShapes.length > 0
    function askForBlur() {
        if (hasGlass)
            window.BackgroundEffect.blurRegion = blur;
    }
    onHasGlassChanged: askForBlur()
    Component.onCompleted: askForBlur()
}
