pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "media.js" as Media

// The pill's equalizer: bars that jump to new heights every
// Tokens.durationEqStep while `playing`, so it costs a few frames a second,
// never one each refresh (no animation on the heights). Paused, they lie
// flat in fgDim. The steps run only while `running` and the bars are shown.
Row {
    id: equalizer

    property bool playing: false
    property bool running: true
    // Bars that stand still at half height while playing, for screenshots
    // that mustn't change.
    property bool still: false

    readonly property list<int> flat: Media.equalizer(Tokens.mediaEqBars, Tokens.mediaEqLow, Tokens.mediaEqLow, Math.random)
    readonly property list<int> middle: Media.equalizer(Tokens.mediaEqBars, Tokens.mediaEqLow, Tokens.mediaEqHigh, () => 0.5)
    property list<int> steps: middle
    readonly property list<int> heights: !playing ? flat : still ? middle : steps

    spacing: Tokens.mediaEqGap
    height: Tokens.mediaEqHigh

    Timer {
        interval: Tokens.durationEqStep
        repeat: true
        triggeredOnStart: true
        running: equalizer.playing && equalizer.running && !equalizer.still && equalizer.visible
        onTriggered: equalizer.steps = Media.equalizer(Tokens.mediaEqBars, Tokens.mediaEqLow, Tokens.mediaEqHigh, Math.random)
    }

    Repeater {
        model: equalizer.heights

        Rectangle {
            required property int modelData
            y: equalizer.height - height
            width: Tokens.mediaEqBarWidth
            height: modelData
            radius: width / 2
            color: equalizer.playing ? Tokens.primary : Tokens.fgDim
        }
    }
}
