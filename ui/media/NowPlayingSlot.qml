pragma ComponentBehavior: Bound

import QtQuick
import "../bar"
import "../theme"
import "service"

// The now-playing pill in the bar.left row (view `media.pill`). It takes
// room only while something plays, as wide as it is closed.
BarSlot {
    id: slot

    readonly property NowPlaying nowPlaying: loader.item as NowPlaying

    pill: nowPlaying
    closedWidth: nowPlaying ? nowPlaying.pillWidth * nowPlaying.shownT : 0
    panels: nowPlaying ? [nowPlaying] : []

    Loader {
        id: loader
        y: Tokens.barGap
        active: MediaService.ready
        sourceComponent: NowPlaying {
            id: pill
            // Placed itself, not by the Loader: its blur and clicks only
            // follow its own moves.
            x: slot.slotX
            media: MediaService.media
            still: slot.bar.stillEqualizer
            onControlRequested: (player, action) => MediaService.control(player, action)
            onSeekRequested: (player, positionUs) => MediaService.seek(player, positionUs)
            onShowRequested: player => MediaService.show(player)
            onFocusRequested: player => MediaService.focus(player)
            onWorkspaceAsked: player => MediaService.window(player, name => {
                    if (pill.playerId === player)
                        pill.workspace = name;
                })
        }
    }
}
