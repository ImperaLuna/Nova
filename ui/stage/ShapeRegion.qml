import Quickshell

// The region one drawn item covers, rounded by its `radius` like it's drawn,
// less the ellipse filling its `cutout` when it has one (StagePanel
// `parts`): a delegate for Variants over a list of items, whose instances
// make up an input mask or blur region.
Region {
    id: region

    required property var modelData
    item: modelData
    radius: modelData.radius

    Region {
        item: region.modelData.cutout ?? null
        shape: RegionShape.Ellipse
        intersection: Intersection.Subtract
    }
}
