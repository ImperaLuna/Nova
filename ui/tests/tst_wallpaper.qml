pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../wallpaper"
import "../wallpaper/carousel.js" as Carousel

// The wallpaper carousel with a folder in the shape novad lists it: where
// it starts, looping past either end, the keys, picking and putting the
// old image back. The root is an Item: a TestCase root gets no key events.
Item {
    id: stage
    width: 1280
    height: 800

    readonly property string image: Qt.resolvedUrl("data/frame-400x300.png").toString().replace("file://", "")
    // Every name a copy of the one test image, as far as the carousel
    // knows: it only ever shows what it's handed.
    readonly property var folder: ({
            folder: "/home/luna/Pictures/Wallpapers",
            images: ["/w/aurora.png", "/w/dusk.png", stage.image, "/w/tide.png"]
        })

    TestCase {
        name: "Carousel"

        function test_it_wraps_round_both_ends() {
            compare(Carousel.wrap(4, 4), 0);
            compare(Carousel.wrap(-1, 4), 3);
            compare(Carousel.wrap(-9, 4), 3);
        }

        function test_cards_sit_the_short_way_round() {
            compare(Carousel.offset(0, 0, 10), 0);
            compare(Carousel.offset(9, 0, 10), -1, "the last image is left of the first");
            compare(Carousel.offset(1, 9, 10), 2, "past the end, the first ones come next");
            compare(Carousel.offset(2, 10.5, 10), 1.5, "however far the deck has gone round");
        }

        function test_a_short_loop_never_shows_a_card_twice() {
            compare(Carousel.shown(0, 10, 3), 1);
            compare(Carousel.shown(3, 10, 3), 1);
            compare(Carousel.shown(4, 10, 3), 0, "only three either side");
            compare(Carousel.shown(1, 3, 3), 1, "three images: one either side");
            compare(Carousel.shown(1.5, 3, 3), 0, "gone before it jumps to the other side");
            compare(Carousel.shown(1, 2, 3), 0, "two images: one at a time");
            compare(Carousel.shown(0, 1, 3), 1);
        }

        function test_it_starts_on_the_screens_image() {
            compare(Carousel.startIndex(["/a.png", "/b.png"], "/b.png"), 1);
            compare(Carousel.startIndex(["/a.png", "/b.png"], "/elsewhere.png"), 0);
            compare(Carousel.fileName("/w/dusk.png"), "dusk.png");
        }
    }

    TestCase {
        name: "WallpaperCarousel"
        when: windowShown

        Component {
            id: carouselComponent
            WallpaperCarousel {
                screenWidth: stage.width
                screenHeight: stage.height
                outputName: "DP-1"
            }
        }

        SignalSpy {
            id: loads
            signalName: "loadRequested"
        }

        SignalSpy {
            id: picks
            signalName: "chosen"
        }

        // Open, with the folder listed and DP-2 showing `shown`.
        function make(shown) {
            const carousel = createTemporaryObject(carouselComponent, stage);
            loads.clear();
            picks.clear();
            loads.target = carousel;
            picks.target = carousel;
            carousel.open = true;
            compare(loads.count, 1, "opening lists the folder");
            carousel.loaded(null, stage.folder, shown ?? {
                path: "/w/dusk.png",
                outputs: {
                    "DP-2": "/w/tide.png"
                }
            });
            return carousel;
        }

        function test_it_opens_on_the_image_this_screen_shows() {
            const carousel = make();
            compare(carousel.pickedPath, "/w/dusk.png");
            compare(carousel.current, "/w/dusk.png");
            compare(carousel.position, 1, "it starts there without moving");
            compare(carousel.previewPath, "/w/dusk.png");
        }

        function test_a_screen_with_its_own_image_opens_on_that() {
            const carousel = createTemporaryObject(carouselComponent, stage, {
                outputName: "DP-2"
            });
            carousel.open = true;
            carousel.loaded(null, stage.folder, {
                path: "/w/dusk.png",
                outputs: {
                    "DP-2": "/w/tide.png"
                }
            });
            compare(carousel.pickedPath, "/w/tide.png");
        }

        function test_the_keys_loop_round_past_either_end() {
            const carousel = make();
            keyClick(Qt.Key_Left);
            keyClick(Qt.Key_Left);
            compare(carousel.pickedPath, "/w/tide.png", "left of the first is the last");
            compare(carousel.previewPath, "/w/tide.png", "the screen previews it");
            keyClick(Qt.Key_Right);
            compare(carousel.pickedPath, "/w/aurora.png", "right of the last is the first");
            tryCompare(carousel, "position", 0, 1000, "the deck moves on round, not back");
        }

        function test_enter_keeps_it_on_every_screen_or_this_one() {
            const carousel = make();
            keyClick(Qt.Key_Right);
            keyClick(Qt.Key_Tab);
            verify(!carousel.everywhere);
            keyClick(Qt.Key_Return);
            compare(picks.count, 1);
            compare(picks.signalArguments[0][0], stage.image);
            compare(picks.signalArguments[0][1], false);
            keyClick(Qt.Key_Right);
            compare(carousel.pickedPath, stage.image, "nothing moves while it's handed over");
            carousel.applied(null);
            tryCompare(carousel, "open", false, 3000, "it closes once awww has faded it in");
        }

        function test_a_failed_pick_says_why_and_stays_open() {
            const carousel = make();
            keyClick(Qt.Key_Return);
            carousel.applied("can't read /w/dusk.png");
            verify(carousel.open);
            verify(!carousel.handingOver);
            compare(carousel.problem, "can't read /w/dusk.png");
            keyClick(Qt.Key_Right);
            compare(carousel.problem, "", "moving on clears it");
        }

        function test_esc_puts_the_old_image_back() {
            const carousel = make();
            keyClick(Qt.Key_Right);
            keyClick(Qt.Key_Escape);
            verify(!carousel.open);
            compare(picks.count, 0, "nothing was set");
            tryCompare(carousel, "count", 0, 2000, "once it's gone, it holds no images");
        }

        function test_an_empty_folder_picks_nothing() {
            const carousel = make();
            carousel.open = false;
            carousel.open = true;
            carousel.loaded(null, {
                folder: "/home/luna/Pictures/Wallpapers",
                images: []
            }, {});
            keyClick(Qt.Key_Right);
            keyClick(Qt.Key_Return);
            compare(picks.count, 0);
            compare(carousel.picked, -1);
        }
    }
}
