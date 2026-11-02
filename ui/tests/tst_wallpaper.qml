pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../wallpaper"
import "../wallpaper/carousel.js" as Carousel
import "../daemon/protocol.js" as Protocol

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
            compare(Carousel.shown(1, 2, 3), 1, "two images: the other rests on the right");
            compare(Carousel.shown(-0.75, 2, 3), 0.5, "and fades out leaving on the left");
            compare(Carousel.shown(2, 4, 3), 1, "four: one rests halfway, on the right");
            compare(Carousel.shown(-2, 4, 3), 0, "never on both sides");
            compare(Carousel.shown(0, 1, 3), 1);
        }

        function test_screens_go_left_to_right() {
            const order = Carousel.screenOrder([
                {
                    name: "DP-2",
                    x: 2560,
                    y: 0,
                    width: 1920,
                    height: 1080
                },
                {
                    name: "DP-1",
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 1440
                }
            ]);
            compare(order.map(screen => screen.name), ["DP-1", "DP-2"]);
            compare(order[1].width, 1920);
        }

        function test_each_screen_shows_its_own_or_the_shared_image() {
            compare(Carousel.showing(["DP-1", "DP-2"], {
                path: "/w/dusk.png",
                outputs: {
                    "DP-2": "/w/tide.png"
                }
            }), {
                "DP-1": "/w/dusk.png",
                "DP-2": "/w/tide.png"
            });
            compare(Carousel.showing(["DP-1"], {}), {
                "DP-1": ""
            });
        }

        function test_tab_goes_round_every_screen_then_each() {
            compare(Carousel.nextScreen("", ["DP-1", "DP-2"], 1), "DP-1");
            compare(Carousel.nextScreen("DP-2", ["DP-1", "DP-2"], 1), "", "back to every screen");
            compare(Carousel.nextScreen("", ["DP-1", "DP-2"], -1), "DP-2");
            compare(Carousel.nextScreen("HDMI-A-1", ["DP-1", "DP-2"], 1), "DP-1", "a screen that's gone");
        }

        function test_picks_become_as_few_calls_as_keep_them() {
            const showing = {
                "DP-1": "/w/dusk.png",
                "DP-2": "/w/tide.png"
            };
            compare(Carousel.requests(Object.assign({}, showing), showing), [], "nothing changed");
            compare(Carousel.requests({
                "DP-1": "/w/a.png",
                "DP-2": "/w/a.png"
            }, showing), [
                {
                    path: "/w/a.png",
                    output: null
                }
            ], "the same image everywhere: one call");
            compare(Carousel.requests({
                "DP-1": "/w/a.png",
                "DP-2": "/w/tide.png"
            }, showing), [
                {
                    path: "/w/a.png",
                    output: "DP-1"
                }
            ], "only what changed");
            compare(Carousel.requests({
                "DP-1": "/w/a.png",
                "DP-2": "/w/b.png"
            }, showing).map(call => call.output), ["DP-1", "DP-2"]);
            compare(Carousel.requests({
                "DP-1": "/w/a.png",
                "DP-2": ""
            }, {
                "DP-1": "/w/dusk.png",
                "DP-2": ""
            }), [
                {
                    path: "/w/a.png",
                    output: "DP-1"
                }
            ], "a screen showing nothing keeps nothing");
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
                screens: [
                    {
                        name: "DP-1",
                        width: 1280,
                        height: 800
                    },
                    {
                        name: "DP-2",
                        width: 1920,
                        height: 1080
                    }
                ]
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
            carousel.loaded(carousel.openings, null, stage.folder, shown ?? {
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
            carousel.loaded(carousel.openings, null, stage.folder, {
                path: "/w/dusk.png",
                outputs: {
                    "DP-2": "/w/tide.png"
                }
            });
            compare(carousel.pickedPath, "/w/tide.png");
        }

        // The cards showing, by where they sit from the middle.
        function cards(item: Item): var {
            let found = [];
            for (const child of item.children) {
                if (child.seen !== undefined && child.visible)
                    found.push(child);
                found = found.concat(cards(child));
            }
            return found.sort((a, b) => a.away - b.away);
        }

        // The screen names marked on the cards in sight.
        function marks(item: Item): var {
            let found = [];
            for (const child of item.children) {
                if (child.marks !== undefined && child.visible)
                    found = found.concat(child.marks);
                found = found.concat(marks(child));
            }
            return found;
        }

        function test_two_images_show_both_once() {
            const carousel = make();
            carousel.loaded(carousel.openings, null, {
                folder: "/home/luna/Pictures/Wallpapers",
                images: ["/w/aurora.png", "/w/dusk.png"]
            }, {
                path: "/w/dusk.png"
            });
            compare(cards(carousel).map(card => card.modelData), ["/w/dusk.png", "/w/aurora.png"]);
            keyClick(Qt.Key_Right);
            // Exactly: tryCompare passes within 0.00001, a frame before the
            // deck rests, while the card leaving left still shows a little.
            tryVerify(() => carousel.position === 2, 1000, "the deck comes to rest");
            compare(cards(carousel).map(card => card.modelData), ["/w/aurora.png", "/w/dusk.png"], "round again, still each once");
            const arrived = cards(carousel)[1];
            verify(arrived.opacity < 1, "the card that came round fades in");
            verify(carousel.moving, "and the stage draws it doing so");
            tryCompare(arrived, "opacity", 1, 1000);
            tryCompare(carousel, "arrivals", 0, 1000);
        }

        function test_focus_moving_behind_it_closes_it() {
            const carousel = make();
            carousel.compositorFocus = "";
            verify(carousel.open, "focus not known: nothing to go by");
            keyClick(Qt.Key_Right);
            carousel.compositorFocus = Protocol.focusKey({
                id: 2,
                active_window: "42"
            });
            verify(!carousel.open, "Alt+Tab closes it, as Esc does");
            compare(picks.count, 0, "nothing was set");
            carousel.open = true;
            carousel.compositorFocus = Protocol.focusKey({
                id: 3
            });
            verify(!carousel.open, "and so does Super+Tab to an empty workspace");
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

        function test_enter_keeps_the_pick_on_every_screen() {
            const carousel = make();
            keyClick(Qt.Key_Right);
            compare(carousel.previews, {
                "DP-2": stage.image
            }, "the other screen previews it too");
            keyClick(Qt.Key_Return);
            compare(picks.count, 1);
            compare(picks.signalArguments[0][0], [
                {
                    path: stage.image,
                    output: null
                }
            ]);
            keyClick(Qt.Key_Right);
            compare(carousel.pickedPath, stage.image, "nothing moves while it's handed over");
            carousel.applied(null);
            tryCompare(carousel, "open", false, 3000, "it closes once awww has faded it in");
        }

        function test_tab_picks_for_one_screen_at_a_time() {
            const carousel = make();
            keyClick(Qt.Key_Tab);
            compare(carousel.choosingFor, "DP-1");
            keyClick(Qt.Key_Right);
            compare(carousel.previewPath, stage.image);
            compare(carousel.previews, {}, "DP-2 keeps its own");
            keyClick(Qt.Key_Tab);
            compare(carousel.choosingFor, "DP-2");
            compare(carousel.pickedPath, "/w/tide.png", "the deck goes to what DP-2 shows");
            keyClick(Qt.Key_Right);
            compare(carousel.pickedPath, "/w/aurora.png", "round past the end");
            compare(carousel.previews, {
                "DP-2": "/w/aurora.png"
            });
            compare(carousel.previewPath, stage.image, "this screen keeps its pick");
            compare(marks(carousel), ["DP-1"], "DP-1's pick says so in the deck");
            keyClick(Qt.Key_Tab);
            compare(carousel.choosingFor, "", "then every screen again");
            compare(carousel.pickedPath, stage.image, "starting from this one's pick");
            keyClick(Qt.Key_Return);
            compare(picks.signalArguments[0][0], [
                {
                    path: stage.image,
                    output: "DP-1"
                },
                {
                    path: "/w/aurora.png",
                    output: "DP-2"
                }
            ], "each screen's own pick, until the deck moves");
        }

        function test_enter_with_nothing_changed_just_closes() {
            const carousel = make();
            keyClick(Qt.Key_Return);
            verify(!carousel.open);
            compare(picks.count, 0);
        }

        function test_enter_keeps_the_middle_card_where_nothing_from_the_folder_shows() {
            const carousel = make({
                path: "/elsewhere/kept.png"
            });
            compare(carousel.pickedPath, "/w/aurora.png", "the deck starts on the first image");
            keyClick(Qt.Key_Tab);
            keyClick(Qt.Key_Return);
            compare(picks.signalArguments[0][0], [
                {
                    path: "/w/aurora.png",
                    output: "DP-1"
                }
            ], "only on the screen it picks for: DP-2 keeps its own");
        }

        function test_one_screen_has_no_screens_to_pick_from() {
            const carousel = createTemporaryObject(carouselComponent, stage, {
                screens: []
            });
            carousel.open = true;
            carousel.loaded(carousel.openings, null, stage.folder, {
                path: "/w/dusk.png"
            });
            keyClick(Qt.Key_Tab);
            compare(carousel.choosingFor, "");
            compare(carousel.screenNames, ["DP-1"]);
        }

        function test_a_screen_going_away_leaves_the_picker_without_errors() {
            const carousel = make();
            failOnWarning(/TypeError/);
            carousel.screens = [carousel.screens[0]];
            compare(carousel.screenNames, ["DP-1"]);
        }

        function test_a_failed_pick_says_why_and_stays_open() {
            const carousel = make();
            keyClick(Qt.Key_Right);
            keyClick(Qt.Key_Return);
            carousel.applied("can't read the image");
            verify(carousel.open);
            verify(!carousel.handingOver);
            compare(carousel.problem, "can't read the image");
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
            carousel.loaded(carousel.openings, null, stage.folder, {});
            compare(carousel.count, 0, "a listing that comes after it closed is dropped");
        }

        function test_a_listing_for_an_earlier_opening_is_dropped() {
            const carousel = make();
            const first = carousel.openings;
            carousel.open = false;
            carousel.open = true;
            carousel.loaded(carousel.openings, null, stage.folder, {});
            keyClick(Qt.Key_Right);
            carousel.loaded(first, null, stage.folder, {
                path: "/w/tide.png"
            });
            compare(carousel.pickedPath, "/w/dusk.png", "the late answer doesn't move it back");
        }

        function test_closed_it_takes_no_clicks() {
            const carousel = make();
            verify(carousel.visible);
            carousel.open = false;
            tryCompare(carousel, "visible", false, 2000, "gone once it's faded out");
            carousel.open = true;
            verify(carousel.visible);
            carousel.loaded(carousel.openings, null, stage.folder, {
                path: "/w/dusk.png"
            });
            keyClick(Qt.Key_Right);
            compare(carousel.pickedPath, stage.image, "back, it takes the keys");
        }

        function test_a_pick_answered_after_esc_doesnt_close_it_again() {
            const carousel = make();
            keyClick(Qt.Key_Right);
            keyClick(Qt.Key_Return);
            keyClick(Qt.Key_Escape);
            carousel.open = true;
            carousel.applied(null);
            wait(1500);
            verify(carousel.open, "the answer was for the pick put back");
        }

        function test_an_empty_folder_picks_nothing() {
            const carousel = make();
            carousel.open = false;
            carousel.open = true;
            carousel.loaded(carousel.openings, null, {
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
