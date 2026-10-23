import Quickshell

// The region one drawn item covers, rounded by its `radius` like it's drawn:
// a delegate for Variants over a list of items, whose instances make up an
// input mask or blur region.
Region {
    required property var modelData
    item: modelData
    radius: modelData.radius
}
