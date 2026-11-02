pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../compositor"
import "../stage"
import "../daemon/protocol.js" as Protocol
import "carousel.js" as Carousel
import "service"

// The wallpaper carousel over the whole screen, in the stage.panels host
// (view `wallpaper.carousel`), while its service is ready. While another
// screen's carousel is open, this screen shows that one's pick for it.
Scope {
    id: slot

    // The screen's stage (Stage.qml).
    required property var stage
    readonly property list<StagePanel> panels: loader.item ? [loader.item] : []

    LazyLoader {
        id: loader
        active: WallpaperService.ready
        component: WallpaperCarousel {
            id: carousel
            screenWidth: slot.stage.output.width
            screenHeight: slot.stage.output.height
            outputName: slot.stage.output.name
            screens: Carousel.screenOrder(Quickshell.screens)
            compositorFocus: Protocol.focusKey(Compositor.focusedWorkspace)
            onLoadRequested: opening => WallpaperService.load((error, listing, shown) => carousel.loaded(opening, error, listing, shown))
            onChosen: requests => WallpaperService.setAll(requests, error => carousel.applied(error))
        }
    }

    // What the carousel previews while it's open or closing here, for the
    // other screens. Opening it here closes it elsewhere, which may still be
    // fading: an open one's preview wins, and a closing one only changes or
    // clears its own.
    readonly property var preview: {
        const carousel = loader.item as WallpaperCarousel;
        if (!carousel || !carousel.shown)
            return null;
        return {
            paths: carousel.previews,
            choosingFor: carousel.choosingFor,
            output: slot.stage.output.name,
            shown: carousel.open
        };
    }
    onPreviewChanged: {
        const ours = WallpaperService.preview?.output === slot.stage.output.name;
        if (preview && (preview.shown || ours || !WallpaperService.preview))
            WallpaperService.preview = preview;
        else if (!preview && ours)
            WallpaperService.preview = null;
    }

    // Another screen's carousel previews this screen's pick here, and
    // marks it while it picks for it.
    LazyLoader {
        readonly property var shownPreview: WallpaperService.preview
        active: shownPreview !== null && shownPreview.output !== slot.stage.output.name
        component: WallpaperPreview {
            screen: slot.stage.screen
            path: WallpaperService.preview?.paths[slot.stage.output.name] ?? ""
            shown: WallpaperService.preview?.shown ?? false
            picking: WallpaperService.preview?.choosingFor === slot.stage.output.name
        }
    }
}
