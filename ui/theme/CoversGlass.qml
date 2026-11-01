import QtQuick

// Marks one of Nova's own windows outside the stage (pop-ups, the volume
// capsule, capture tools) as on screen while `shown`: copies of the screen
// for glass wait until none is (GlassBackdrops `covered`), or glass would
// copy Nova itself and keep that after it's gone.
QtObject {
    id: cover

    property bool shown: false

    onShownChanged: GlassBackdrops.cover(cover, shown)
    Component.onCompleted: GlassBackdrops.cover(cover, shown)
    Component.onDestruction: GlassBackdrops.cover(cover, false)
}
