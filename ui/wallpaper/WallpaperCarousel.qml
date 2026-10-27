pragma ComponentBehavior: Bound

import QtQuick
import "../stage"
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "carousel.js" as Carousel

// The wallpaper carousel: over the whole screen, which previews the picked
// image, a deck of the wallpaper folder's images in the middle, the picked
// one facing you and the others tilted away either side. It loops: past
// the last image comes the first again. It picks for every screen at once,
// or (Tab, or a click in the screen picker under the deck) for one screen,
// each screen previewing its own pick. Left and Right (or the wheel) move
// through the images, Enter (or a click on the middle card) keeps every
// screen's pick, Esc (or a click beside the deck) puts them all back.
// Opened by `nova toggle wallpaper`. It gets its data and acts through its
// properties, signals and functions (the stage wires WallpaperService to
// them), so the QML tests can drive it.
StagePanel {
    id: carousel

    // The screen it covers.
    required property real screenWidth
    required property real screenHeight
    required property string outputName
    // Every screen, {name, width, height} left to right (Carousel.screenOrder);
    // none counts as only this one.
    property var screens: []
    // Where the compositor's focus is: the focused workspace and the
    // window on it (Protocol.focusKey), "" while it isn't known.
    property string compositorFocus: ""

    // The images to pick from (absolute paths), the folder they're in, what
    // each screen shows now and what it would show (by name, "" for none).
    property var images: []
    property string folder: ""
    property var showing: ({})
    property var picks: ({})
    readonly property var screenNames: screens.length > 0 ? screens.map(screen => screen.name) : [outputName]
    // The screen picked for, "" for every one; and so the screens moving
    // the deck picks for.
    property string choosingFor: ""
    readonly property var covered: choosingFor === "" ? screenNames : [choosingFor]
    // What this screen shows now.
    readonly property string current: showing[outputName] ?? ""
    // Waiting for novad to list the folder, and why it couldn't, or why the
    // pick didn't work.
    property bool loading: false
    property string problem: ""
    // Picked, and handed over to awww: the preview stays while awww fades
    // the image in under it.
    property bool handingOver: false

    // Opened, for the `opening`th time: novad's listing goes to loaded().
    signal loadRequested(int opening)
    property int openings: 0
    // Enter: the set_wallpaper calls ({path, output}, Carousel.requests)
    // that keep the picks; novad's answer goes to applied().
    signal chosen(var requests)

    // Where the picked card is, counted without wrapping (each step adds or
    // takes one), and where the deck is on its way there.
    property int target: 0
    property real position: target
    Behavior on position {
        id: stepping
        NumberAnimation {
            id: stepMotion
            duration: Tokens.durationCarouselStep
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }
    // Cards fading in where they came round to (see the deck).
    property int arrivals: 0
    readonly property int count: images.length
    readonly property int picked: count > 0 ? Carousel.wrap(target, count) : -1
    readonly property string pickedPath: picked >= 0 ? images[picked] : ""
    // What Enter would keep: the picks, and the middle card on a screen it
    // picks for that shows nothing from the folder yet.
    readonly property var kept: {
        const kept = Object.assign({}, picks);
        for (const name of covered) {
            if (pickedPath !== "" && !images.includes(kept[name]))
                kept[name] = pickedPath;
        }
        return kept;
    }
    // What this screen previews: its pick, or its own image until the
    // folder's listed. The other screens preview theirs (`previews`, by
    // name) where they'd change.
    readonly property string previewPath: kept[outputName] || current
    readonly property var previews: {
        const changed = {};
        for (const name of screenNames) {
            if (name !== outputName && kept[name] && kept[name] !== showing[name])
                changed[name] = kept[name];
        }
        return changed;
    }

    readonly property real cardWidth: Math.round(screenWidth * Tokens.carouselCardShare)
    readonly property real cardHeight: Math.round(cardWidth * screenHeight / Math.max(1, screenWidth))
    readonly property real deckY: Math.round((screenHeight - cardHeight) / 2)

    name: Protocol.FEATURE_WALLPAPER
    placement: "center"
    wantsKeyboard: true
    // It covers the screen with the preview, so there's nothing behind it
    // for the compositor to blur.
    glass: false
    x: 0
    y: 0
    width: screenWidth
    height: screenHeight
    area: Qt.rect(0, 0, screenWidth, screenHeight)

    readonly property Morph morph: Morph {
        open: carousel.open
    }
    moving: morph.moving || stepMotion.running || arrivals > 0 || preview.moving || deckFade.running || handOver.running
    opacity: morph.t
    // Closed, it takes no clicks and covers nothing (StagePanels `shapes`).
    visible: shown
    // Opening, the deck rises into place as its cards spread out.
    readonly property real spread: morph.lerp(Tokens.carouselEnterSpread, 1)
    readonly property real rise: morph.lerp(Tokens.carouselEnterRise, 0)

    // Focus moving behind it (Alt+Tab, Super+Tab to another workspace, a
    // window opening) closes it as Esc does, rather than leaving it over
    // windows switching behind it.
    onCompositorFocusChanged: {
        if (compositorFocus !== "")
            open = false;
    }

    onOpenChanged: {
        console.info(`nova: wallpaper carousel ${open ? "opened" : "closed"}`);
        handOver.stop();
        // Kept as it closes, so a picked deck doesn't come back as it fades.
        if (!open)
            return;
        handingOver = false;
        images = [];
        problem = "";
        choosingFor = "";
        loading = true;
        keys.forceActiveFocus();
        loadRequested(++openings);
    }

    // Once it's closed, its images go, and the preview's picture with them.
    readonly property bool shown: open || moving
    onShownChanged: {
        if (shown)
            return;
        images = [];
        showing = {};
        picks = {};
    }

    // novad's answer to loadRequested(opening): the folder's {folder,
    // images} and the wallpapers shown now ({path, outputs}), or why it
    // couldn't say.
    function loaded(opening: int, error: var, listing: var, shown: var) {
        // An answer for an opening that's closed since.
        if (!open || opening !== openings)
            return;
        loading = false;
        if (error) {
            problem = error;
            return;
        }
        folder = listing.folder;
        showing = Carousel.showing(screenNames, shown);
        // Straight to the screen's own image, without moving there.
        stepping.enabled = false;
        target = Carousel.startIndex(listing.images, current);
        stepping.enabled = true;
        images = listing.images;
        picks = Object.assign({}, showing);
    }

    // Moves the deck `by` cards, round past either end.
    function step(by: int) {
        if (by === 0 || count === 0 || handingOver)
            return;
        problem = "";
        target += by;
        const picked = Object.assign({}, picks);
        for (const name of covered)
            picked[name] = images[Carousel.wrap(target, count)];
        picks = picked;
    }

    // Picks for screen `name` from now on, "" for every one: the deck moves
    // round to what it would show.
    function chooseFor(name: string) {
        if (screenNames.length < 2 || handingOver)
            return;
        choosingFor = name;
        problem = "";
        const at = images.indexOf(picks[covered[0]]);
        if (at >= 0)
            target += Math.round(Carousel.offset(at, target, count));
    }

    // Keeps every screen's pick (see chosen); with none changed, it closes.
    function choose() {
        if (count === 0 || handingOver)
            return;
        const requests = Carousel.requests(kept, showing);
        if (requests.length === 0) {
            open = false;
            return;
        }
        handingOver = true;
        chosen(requests);
    }

    // novad's answer to chosen: closes once awww has faded the images in.
    function applied(error: var) {
        // An answer for a pick that was put back with Esc since.
        if (!handingOver)
            return;
        if (error) {
            handingOver = false;
            problem = error;
            return;
        }
        handOver.restart();
    }

    Timer {
        id: handOver
        interval: Tokens.durationWallpaperHandOver
        onTriggered: carousel.open = false
    }

    // The preview, and a shade under the deck so its label reads.
    PreviewImage {
        id: preview
        anchors.fill: parent
        path: carousel.previewPath
        pixels: Qt.size(carousel.screenWidth * Screen.devicePixelRatio, carousel.screenHeight * Screen.devicePixelRatio)
    }

    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop {
                position: Tokens.carouselShadeFrom
                color: "transparent"
            }
            GradientStop {
                position: 1
                color: Tokens.carouselShade
            }
        }
    }

    PickingFor {
        anchors.fill: parent
        shown: carousel.choosingFor === carousel.outputName
    }

    // A click beside the deck puts the old image back; the wheel moves it.
    MouseArea {
        anchors.fill: parent
        onClicked: carousel.open = false
        property real wheelRest: 0
        onWheel: wheel => {
            const delta = Math.abs(wheel.angleDelta.x) > Math.abs(wheel.angleDelta.y) ? -wheel.angleDelta.x : wheel.angleDelta.y;
            wheelRest -= delta;
            const notches = Math.trunc(wheelRest / Tokens.wheelNotch);
            wheelRest -= notches * Tokens.wheelNotch;
            carousel.step(notches);
        }
    }

    // The keys, while it's open.
    Item {
        id: keys
        focus: true
        Keys.onPressed: event => {
            switch (event.key) {
            case Qt.Key_Left:
            case Qt.Key_Up:
                carousel.step(-1);
                break;
            case Qt.Key_Right:
            case Qt.Key_Down:
                carousel.step(1);
                break;
            case Qt.Key_Return:
            case Qt.Key_Enter:
                carousel.choose();
                break;
            case Qt.Key_Tab:
            case Qt.Key_Backtab:
                carousel.chooseFor(Carousel.nextScreen(carousel.choosingFor, carousel.screenNames, event.key === Qt.Key_Tab ? 1 : -1));
                break;
            case Qt.Key_Escape:
                carousel.open = false;
                break;
            default:
                return;
            }
            event.accepted = true;
        }
    }

    // The deck and its label, which fade out once an image is picked.
    Item {
        id: deck
        width: carousel.width
        height: carousel.height
        y: carousel.rise
        opacity: carousel.handingOver ? 0 : 1
        Behavior on opacity {
            NumberAnimation {
                id: deckFade
                duration: Tokens.durationFast
            }
        }

        Repeater {
            model: carousel.images

            // Every image has a place round the loop; only the cards near
            // the middle are made.
            Item {
                id: slot

                required property string modelData
                required property int index
                readonly property real away: Carousel.offset(index, carousel.position, carousel.count)
                readonly property real near: Math.min(Math.abs(away), 1)
                readonly property real far: Math.max(Math.abs(away) - 1, 0)
                readonly property real seen: Carousel.shown(away, carousel.count, Tokens.carouselSides)

                width: carousel.cardWidth
                height: carousel.cardHeight
                x: (carousel.width - width) / 2 + Math.sign(away) * (near * Tokens.carouselNeighbour + far * Tokens.carouselStep) * width * carousel.spread
                y: carousel.deckY
                z: -Math.abs(away)
                scale: 1 - near * (1 - Tokens.carouselSideScale) - far * Tokens.carouselDepthScale
                // Follows `seen`, except that a card coming round to rest
                // halfway (the other image of two) fades in there rather
                // than popping up: from frame to frame `seen` only jumps then.
                opacity: 0
                visible: opacity > 0
                Component.onCompleted: opacity = seen
                Component.onDestruction: {
                    if (arrive.running)
                        carousel.arrivals -= 1;
                }
                onSeenChanged: {
                    if (seen - opacity > 0.5) {
                        arrive.to = seen;
                        arrive.restart();
                    } else if (!arrive.running || seen < opacity) {
                        arrive.stop();
                        opacity = seen;
                    }
                }
                NumberAnimation {
                    id: arrive
                    target: slot
                    property: "opacity"
                    duration: Tokens.durationFade
                    onRunningChanged: carousel.arrivals += running ? 1 : -1
                }
                transform: Rotation {
                    origin.x: slot.width / 2
                    origin.y: slot.height / 2
                    axis {
                        x: 0
                        y: 1
                        z: 0
                    }
                    angle: Math.max(-1, Math.min(1, slot.away)) * Tokens.carouselTilt
                }

                Loader {
                    anchors.fill: parent
                    active: slot.visible
                    sourceComponent: WallpaperCard {
                        path: slot.modelData
                        picked: slot.index === carousel.picked
                        now: slot.modelData === carousel.showing[carousel.covered[0]]
                        // The other screens picking it.
                        marks: carousel.screenNames.filter(name => !carousel.covered.includes(name) && carousel.picks[name] === slot.modelData)
                        dim: slot.near * Tokens.carouselSideDim
                    }
                }

                // The middle card keeps its image; another moves there.
                MouseArea {
                    anchors.fill: parent
                    onClicked: {
                        const by = Math.round(slot.away);
                        if (by === 0)
                            carousel.choose();
                        else
                            carousel.step(by);
                    }
                }
            }
        }

        // The picked image's name, where it is in the folder, and where
        // it'd go; or why it couldn't.
        Rectangle {
            visible: carousel.count > 0
            x: Math.round((carousel.width - width) / 2)
            y: carousel.deckY + carousel.cardHeight + Tokens.carouselLabelGap
            width: label.implicitWidth + 2 * Tokens.carouselLabelPad
            height: carousel.screenNames.length > 1 ? Tokens.carouselPickerHeight : Tokens.carouselLabelHeight
            radius: height / 2
            color: Tokens.surface

            // Clicks here don't reach the background, which closes.
            MouseArea {
                anchors.fill: parent
            }

            Row {
                id: label
                anchors.centerIn: parent
                spacing: Tokens.carouselLabelPad

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizePageTitle
                    font.weight: Font.Medium
                    color: carousel.problem !== "" ? Tokens.error : Tokens.fg
                    textFormat: Text.PlainText
                    elide: Text.ElideMiddle
                    width: Math.min(implicitWidth, carousel.cardWidth)
                    text: carousel.problem !== "" ? qsTr("Couldn't set it: %1").arg(carousel.problem) : Carousel.fileName(carousel.pickedPath)
                }

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    font.family: Tokens.fontMono
                    font.pixelSize: Tokens.fontSizeSmall
                    color: Tokens.fgDim
                    text: `${carousel.picked + 1} / ${carousel.count}`
                }

                // Which screen the deck picks for: every one, or one of
                // them drawn to scale with its pick. Not with one screen.
                Row {
                    anchors.verticalCenter: parent.verticalCenter
                    visible: carousel.screenNames.length > 1
                    spacing: Tokens.carouselMiniGap

                    Rectangle {
                        anchors.verticalCenter: parent.verticalCenter
                        width: all.implicitWidth + 2 * Tokens.chipPad
                        height: Tokens.carouselChipHeight
                        radius: height / 2
                        color: carousel.choosingFor === "" ? Tokens.selectedQuiet : "transparent"
                        border.width: carousel.choosingFor === "" ? 0 : 1
                        border.color: Tokens.glassRim

                        Row {
                            id: all
                            anchors.centerIn: parent
                            spacing: Tokens.iconGap

                            LineIcon {
                                anchors.verticalCenter: parent.verticalCenter
                                path: Icons.MONITORS
                                size: Tokens.iconSize
                                color: carousel.choosingFor === "" ? Tokens.fg : Tokens.fgDim
                            }

                            Text {
                                anchors.verticalCenter: parent.verticalCenter
                                font.family: Tokens.fontSans
                                font.pixelSize: Tokens.fontSizeSmall
                                color: carousel.choosingFor === "" ? Tokens.fg : Tokens.fgDim
                                text: qsTr("All")
                            }
                        }

                        MouseArea {
                            anchors.fill: parent
                            onClicked: carousel.chooseFor("")
                        }
                    }

                    Repeater {
                        model: carousel.screens

                        ScreenMini {
                            required property var modelData
                            anchors.verticalCenter: parent.verticalCenter
                            name: modelData.name
                            path: carousel.picks[modelData.name] ?? ""
                            chosen: carousel.choosingFor === modelData.name
                            width: Math.round(Tokens.carouselMiniWidth * modelData.width / Math.max(...carousel.screens.map(screen => screen.width)))
                            height: Math.round(width * modelData.height / Math.max(1, modelData.width))
                            onClicked: carousel.chooseFor(modelData.name)
                        }
                    }
                }

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    visible: carousel.screenNames.length > 1
                    font.family: Tokens.fontMono
                    font.pixelSize: Tokens.fontSizeTiny
                    color: Tokens.fgDim
                    text: qsTr("Tab")
                }
            }
        }

        // No images: where to put some, or why the folder couldn't be read.
        Rectangle {
            visible: carousel.count === 0 && !carousel.loading
            anchors.centerIn: parent
            width: Tokens.carouselNoteWidth
            height: note.implicitHeight + 2 * Tokens.panelPad
            radius: Tokens.radiusPanel
            color: Tokens.surface

            MouseArea {
                anchors.fill: parent
            }

            Column {
                id: note
                anchors.centerIn: parent
                width: parent.width - 2 * Tokens.panelPad
                spacing: Tokens.gap

                Text {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeTitle
                    font.weight: Font.DemiBold
                    color: Tokens.fg
                    text: carousel.problem !== "" ? qsTr("Can't show your wallpapers") : qsTr("No wallpapers yet")
                }

                Text {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    lineHeight: Tokens.lineHeightWrapped
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSize
                    color: Tokens.fgDim
                    textFormat: Text.PlainText
                    text: carousel.problem !== "" ? carousel.problem : qsTr("Put pictures in %1, or point [wallpaper] folder in config.toml at another folder.").arg(carousel.folder)
                }
            }
        }
    }
}
