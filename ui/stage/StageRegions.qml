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
    // Rects in window coordinates cut out of the mask: other surfaces that
    // take the clicks there (Stage, IMP-148).
    property list<rect> holes: []

    readonly property Region mask: Region {
        y: stageRegions.offset
        regions: [...inputRegions.instances, ...holeRegions.instances]
    }
    // Only the glass on the window: Hyprland 0.56 blurs the whole surface
    // when the region lies wholly outside it (it clips the region to the
    // surface, and an empty clip means no clip:
    // renderTextureWithBlurInternal in src/render/OpenGL.cpp). The pills
    // sliding out of sight above the stage showed a blurred band as tall as
    // the stage (IMP-180). The protocol clips to the surface anyway, so this
    // costs nothing elsewhere.
    readonly property Region blur: Region {
        Region {
            y: stageRegions.offset
            regions: blurRegions.instances
        }
        Region {
            width: stageRegions.window.width
            height: stageRegions.window.height
            intersection: Intersection.Intersect
        }
    }

    readonly property Variants inputRegions: Variants {
        id: inputRegions
        model: stageRegions.inputShapes
        ShapeRegion {}
    }
    readonly property Variants holeRegions: Variants {
        id: holeRegions
        model: stageRegions.holes
        Region {
            required property rect modelData
            x: modelData.x
            y: modelData.y
            width: modelData.width
            height: modelData.height
            intersection: Intersection.Subtract
        }
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
