pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../launcher"
import "../theme"
import "../launcher/launcher.js" as LauncherLogic

// The launcher with apps in the shape novad sends: what its list shows
// before and while searching, what other providers find, typing, the
// keys, launching and its failures. The root is an Item: a TestCase root gets no key events.
Item {
    id: stage
    width: 1280
    height: 800

    readonly property var apps: [
        {
            id: "firefox.desktop",
            name: "Firefox",
            description: "Web Browser",
            icon: "firefox"
        },
        {
            id: "foot.desktop",
            name: "Foot"
        },
        {
            id: "org.gnome.Nautilus.desktop",
            name: "Files",
            description: "File Manager"
        }
    ]

    TestCase {
        name: "Launcher"
        when: windowShown

        Component {
            id: rowComponent
            LauncherRow {}
        }
        Component {
            id: launcherComponent
            Launcher {
                screenWidth: stage.width
                screenHeight: stage.height
            }
        }

        SignalSpy {
            id: searches
            signalName: "searchRequested"
        }

        SignalSpy {
            id: launches
            signalName: "runRequested"
        }

        function make(props) {
            const launcher = createTemporaryObject(launcherComponent, stage, Object.assign({
                apps: stage.apps,
                pinned: ["foot.desktop", "gone.desktop"],
                recent: ["org.gnome.Nautilus.desktop", "firefox.desktop"]
            }, props ?? {}));
            searches.clear();
            launches.clear();
            searches.target = launcher;
            launches.target = launcher;
            launcher.open = true;
            return launcher;
        }

        function ids(rows) {
            return rows.map(row => row.id);
        }

        function groups(rows) {
            return rows.map(row => row.group);
        }

        function test_recent_apps_then_every_other_app_by_name_until_something_is_typed() {
            const launcher = make();
            compare(ids(launcher.rows), ["org.gnome.Nautilus.desktop", "firefox.desktop", "foot.desktop"]);
            compare(groups(launcher.rows), ["recent", "recent", "apps"]);
            compare(ids(launcher.pinnedApps), ["foot.desktop"], "an app that's gone isn't shown");
            verify(launcher.showsPinned);
        }

        function test_every_app_shows_before_the_first_launch() {
            const launcher = make({
                recent: []
            });
            compare(ids(launcher.rows), ["org.gnome.Nautilus.desktop", "firefox.desktop", "foot.desktop"], "by name: Files, Firefox, Foot");
            compare(groups(launcher.rows), ["apps", "apps", "apps"]);
        }

        function test_typing_searches_and_shows_only_the_latest_matches() {
            const launcher = make();
            keyClick("f");
            keyClick("i");
            compare(searches.count, 2);
            compare(searches.signalArguments[1][0], "fi");
            verify(!launcher.showsPinned);
            launcher.showMatches("f", [
                {
                    id: "foot.desktop",
                    name: [
                        {
                            text: "F",
                            matched: true
                        },
                        {
                            text: "oot"
                        }
                    ]
                }
            ]);
            compare(launcher.rows.length, 0, "an answer to an older query is dropped");
            launcher.showMatches("fi", [
                {
                    id: "org.gnome.Nautilus.desktop",
                    name: [
                        {
                            text: "Fi",
                            matched: true
                        },
                        {
                            text: "les"
                        }
                    ]
                },
                {
                    id: "firefox.desktop",
                    name: [
                        {
                            text: "Fi",
                            matched: true
                        },
                        {
                            text: "refox"
                        }
                    ]
                }
            ]);
            compare(ids(launcher.rows), ["org.gnome.Nautilus.desktop", "firefox.desktop"]);
            compare(groups(launcher.rows), ["results", "results"]);
            compare(launcher.rows[0].runs[0], {
                text: "Fi",
                matched: true
            });
            compare(launcher.rows[0].runs[1].matched, false);
            keyClick(Qt.Key_Backspace);
            keyClick(Qt.Key_Backspace);
            compare(launcher.query, "");
            compare(launcher.rows[0].group, "recent");
        }

        function test_enter_before_the_latest_matches_waits_for_them() {
            const launcher = make();
            const files = [
                {
                    id: "org.gnome.Nautilus.desktop",
                    name: [
                        {
                            text: "Files"
                        }
                    ]
                }
            ];
            keyClick("f");
            launcher.showMatches("f", [
                {
                    id: "foot.desktop",
                    name: [
                        {
                            text: "Foot"
                        }
                    ]
                }
            ]);
            keyClick("i");
            keyClick(Qt.Key_Return, Qt.ShiftModifier);
            compare(launches.count, 0, "foot was the previous query's");
            launcher.showMatches("fi", files);
            compare(launches.count, 1);
            compare(Array.from(launches.signalArguments[0]), ["apps", "org.gnome.Nautilus.desktop", true]);
        }

        function test_closing_drops_the_waiting_launch() {
            const launcher = make();
            keyClick("f");
            keyClick(Qt.Key_Return);
            launcher.open = false;
            launcher.showMatches("f", [
                {
                    id: "foot.desktop",
                    name: [
                        {
                            text: "Foot"
                        }
                    ]
                }
            ]);
            compare(launches.count, 0);
        }

        function test_typing_after_enter_drops_the_waiting_launch() {
            const launcher = make();
            keyClick("f");
            keyClick(Qt.Key_Return);
            keyClick("i");
            launcher.showMatches("fi", [
                {
                    id: "firefox.desktop",
                    name: [
                        {
                            text: "Firefox"
                        }
                    ]
                }
            ]);
            compare(launches.count, 0);
        }

        function test_another_providers_items_show_unpinnable_and_run_through_it() {
            const launcher = make();
            keyClick("=");
            launcher.showMatches("=", [
                {
                    provider: "calculator",
                    id: "2",
                    name: [
                        {
                            text: "2"
                        }
                    ],
                    description: "1 + 1"
                },
                {
                    provider: "apps",
                    id: "foot.desktop",
                    name: [
                        {
                            text: "Foot"
                        }
                    ]
                }
            ]);
            compare(launcher.rows.map(row => row.provider), ["calculator", "apps"]);
            compare(launcher.rows[0].name, "2");
            compare(launcher.rows[0].description, "1 + 1");
            const list = findChild(launcher, "list");
            tryVerify(() => list.itemAtIndex(1) !== null);
            verify(!findChild(list.itemAtIndex(0), "row").pinnable, "only apps can be pinned");
            verify(findChild(list.itemAtIndex(1), "row").pinnable);
            keyClick(Qt.Key_Return);
            compare(Array.from(launches.signalArguments[0]), ["calculator", "2", false]);
            verify(launcher.isLaunching(launcher.rows[0]));
            verify(!launcher.isLaunching(launcher.rows[1]));
        }

        function test_nothing_matching_says_so() {
            const launcher = make();
            keyClick("q");
            launcher.showMatches("q", []);
            compare(launcher.rows.length, 0);
        }

        function test_arrows_go_round_and_enter_launches_the_selected_app() {
            const launcher = make();
            compare(launcher.selected, 0);
            keyClick(Qt.Key_Up);
            compare(launcher.selected, 2, "up from the first goes to the last");
            keyClick(Qt.Key_Down);
            keyClick(Qt.Key_Down);
            compare(launcher.selected, 1);
            keyClick(Qt.Key_Return);
            compare(launches.count, 1);
            compare(Array.from(launches.signalArguments[0]), ["apps", "firefox.desktop", false]);
            compare(launcher.launching, {
                provider: "apps",
                id: "firefox.desktop"
            });
            keyClick(Qt.Key_Return);
            compare(launches.count, 1, "one launch at a time");
            launcher.launched(null);
            tryCompare(launcher, "open", false);
        }

        function test_a_pinned_tile_launching_leaves_the_selected_row_alone() {
            const launcher = make();
            launcher.launch({
                provider: "apps",
                id: "foot.desktop"
            }, false);
            verify(launcher.isLaunching({
                provider: "apps",
                id: "foot.desktop"
            }));
            verify(!launcher.launchingSelected, "the selected Files row isn't the one launching");
        }

        function test_the_selected_row_launching_shows_on_it() {
            const launcher = make();
            keyClick(Qt.Key_Return);
            verify(launcher.launchingSelected);
        }

        function test_a_new_recent_order_shows_the_next_time_it_opens() {
            const launcher = make();
            const before = ids(launcher.rows);
            launcher.pointed(1, Qt.point(10, 40));
            keyClick(Qt.Key_Return);
            // novad moves the launched app to the top before it answers.
            launcher.recent = ["firefox.desktop", "org.gnome.Nautilus.desktop"];
            compare(ids(launcher.rows), before, "held while it launches");
            launcher.launched(null);
            tryCompare(launcher, "open", false);
            compare(ids(launcher.rows), before, "held while it fades out");
            launcher.open = true;
            compare(ids(launcher.rows), ["firefox.desktop", "org.gnome.Nautilus.desktop", "foot.desktop"]);
        }

        function test_a_new_recent_order_shows_at_once_while_nothing_launches() {
            const launcher = make();
            launcher.recent = ["firefox.desktop"];
            compare(ids(launcher.rows), ["firefox.desktop", "org.gnome.Nautilus.desktop", "foot.desktop"]);
        }

        function test_rows_scrolling_under_a_still_pointer_keep_the_selection() {
            const launcher = make();
            launcher.pointed(1, Qt.point(10, 40));
            compare(launcher.selected, 1);
            keyClick(Qt.Key_Down);
            launcher.pointed(0, Qt.point(10, 40));
            compare(launcher.selected, 2, "the pointer didn't move");
            launcher.pointed(0, Qt.point(10, 20));
            compare(launcher.selected, 0);
        }

        function test_shift_enter_asks_for_a_new_window() {
            make();
            keyClick(Qt.Key_Return, Qt.ShiftModifier);
            compare(Array.from(launches.signalArguments[0]), ["apps", "org.gnome.Nautilus.desktop", true]);
        }

        function test_a_failed_launch_says_why_and_stays_open() {
            const launcher = make();
            keyClick(Qt.Key_Return);
            launcher.launched("dolphin isn't installed");
            compare(launcher.problem, "dolphin isn't installed");
            compare(launcher.launching, null);
            verify(launcher.open);
            keyClick("x");
            compare(launcher.problem, "", "typing clears it");
        }

        function test_esc_closes_and_opening_starts_afresh() {
            const launcher = make();
            keyClick("f");
            keyClick(Qt.Key_Escape);
            verify(!launcher.open);
            verify(!launcher.glass, "no blur left behind while it fades");
            launcher.open = true;
            compare(launcher.query, "");
            compare(launcher.selected, 0);
        }

        function test_the_surface_fits_the_tallest_deck() {
            const launcher = make({
                apps: Array.from({
                    length: 20
                }, (_, i) => ({
                            id: `app${i}.desktop`,
                            name: `App ${i}`
                        })),
                pinned: Array.from({
                    length: 20
                }, (_, i) => `app${i}.desktop`),
                recent: []
            });
            tryCompare(launcher.morph, "moving", false);
            verify(launcher.pinnedApps.length < 20, "one row of tiles");
            verify(launcher.shownHeight <= launcher.tallest, `${launcher.shownHeight} > ${launcher.tallest}`);
            verify(launcher.area.y + launcher.area.height >= launcher.y + launcher.height);
        }

        function test_on_a_short_screen_the_list_ends_above_its_edge() {
            const launcher = make({
                screenHeight: 540,
                apps: Array.from({
                    length: 30
                }, (_, i) => ({
                            id: `app${i}.desktop`,
                            name: `App ${i}`
                        })),
                pinned: ["app0.desktop"],
                recent: []
            });
            verify(launcher.openY + launcher.tallest <= 540, `${launcher.openY + launcher.tallest} > 540`);
            verify(launcher.listHeight >= Tokens.launcherRowHeight);
        }
    }

    TestCase {
        name: "LauncherLogic"

        function test_steps_go_round() {
            compare(LauncherLogic.step(0, 3, -1), 2);
            compare(LauncherLogic.step(2, 3, 1), 0);
            compare(LauncherLogic.step(0, 0, 1), 0);
        }

        function test_a_long_name_ends_in_an_ellipsis_inside_its_row() {
            const name = "LibreOffice Impress Presentation Editor and Viewer";
            const row = createTemporaryObject(rowComponent, stage, {
                width: 300,
                app: {
                    id: "impress.desktop",
                    name: name,
                    runs: [
                        {
                            text: "Lib",
                            matched: true
                        },
                        {
                            text: name.slice(3),
                            matched: false
                        }
                    ],
                    description: ""
                }
            });
            const label = findChild(row, "label");
            const last = findChild(row, "nameRun1");
            tryVerify(() => last.truncated, 1000, "the name is cut short");
            verify(last.x + last.width <= label.width, "it ends inside the label");
        }

        function test_a_missing_icon_gets_a_letter_on_a_swatch() {
            compare(LauncherLogic.letter("firefox"), "F");
            compare(LauncherLogic.letter("  7-Zip"), "7");
            compare(LauncherLogic.letter("éclair"), "É");
            compare(LauncherLogic.letter("--"), "?");
            const swatch = LauncherLogic.swatch("org.mozilla.firefox.desktop");
            verify(swatch >= 1 && swatch <= LauncherLogic.SWATCHES);
            compare(LauncherLogic.swatch("org.mozilla.firefox.desktop"), swatch);
        }
    }
}
