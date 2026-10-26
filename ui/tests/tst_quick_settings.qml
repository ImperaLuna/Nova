import QtQuick
import QtTest
import "../quick_settings"
import "../theme"
import "../quick_settings/quick_settings.js" as QuickSettings

// Quick settings' logic, and its pages driven with data in the shapes
// novad and SoundService give them. The root is an Item: a TestCase root
// gets no mouse events.
Item {
    id: stage
    width: 360
    height: 900

    TestCase {
        name: "QuickSettings"
        when: windowShown

        Component {
            id: mainComponent
            MainPage {
                width: 328
                height: 868
            }
        }
        Component {
            id: soundComponent
            SoundPage {
                width: 328
                height: 868
            }
        }
        Component {
            id: resourcesComponent
            ResourcesPage {
                width: 328
                height: 868
            }
        }

        Component {
            id: headerComponent
            Header {}
        }

        Component {
            id: sliderComponent
            QsSlider {
                width: 300
                label: "Firefox"
            }
        }

        SignalSpy {
            id: spy
        }

        function init() {
            Tokens.apply({
                bg: "#1D1D1D",
                fg: "#F5E7C6",
                fgDim: "#9A9A9A",
                primary: "#FD7014",
                secondary: "#BE95FF",
                onAccent: "#1D1D1D"
            });
            spy.target = null;
            spy.signalName = "";
            spy.clear();
        }

        function reading() {
            return {
                cpu: 23.4,
                memory_used: 10093173760,
                memory_total: 34359738368,
                gpu: 18,
                history: [
                    {
                        cpu: 20,
                        memory: 29
                    },
                    {
                        cpu: 48,
                        memory: 30
                    },
                    {
                        cpu: 23,
                        memory: 29
                    }
                ],
                history_size: 36,
                apps: [
                    {
                        name: "firefox",
                        key: "/usr/lib/firefox/firefox",
                        processes: 9,
                        cpu: 6.1,
                        memory: 2400000000,
                        protected: false
                    },
                    {
                        name: "niri",
                        key: "/usr/bin/niri",
                        processes: 1,
                        cpu: 2.3,
                        memory: 310000000,
                        protected: true
                    }
                ]
            };
        }

        // Where shot() saves what a page shows, to look at it; set it by hand
        // while working on the pages.
        property string shotDir: ""
        function shot(item, file) {
            const image = grabImage(item);
            verify(image.width > 0);
            if (shotDir !== "")
                image.save(`${shotDir}/${file}.png`);
        }

        function test_greeting_follows_the_hour() {
            compare(QuickSettings.greeting(5), "Good morning");
            compare(QuickSettings.greeting(11), "Good morning");
            compare(QuickSettings.greeting(12), "Good afternoon");
            compare(QuickSettings.greeting(18), "Good evening");
            compare(QuickSettings.greeting(2), "Good evening");
        }

        function test_logo_prefers_bundled_then_like_then_icon_theme() {
            const url = path => `file://${path}`;
            compare(QuickSettings.logo(null, "logos", url), "");
            compare(QuickSettings.logo({
                id: "nixos"
            }, "logos", url), "logos/nixos.svg");
            compare(QuickSettings.logo({
                id: "mynix",
                id_like: ["nixos"]
            }, "logos", url), "logos/nixos.svg");
            compare(QuickSettings.logo({
                id: "fedora",
                logo: "/icons/fedora-logo-icon.svg"
            }, "logos", url), "file:///icons/fedora-logo-icon.svg");
            compare(QuickSettings.logo({
                id: "fedora"
            }, "logos", url), "", "an icon theme without it shows Nova's mark");
        }

        function test_sizes_read_short() {
            compare(QuickSettings.shortBytes(10093173760), "9.4G");
            compare(QuickSettings.shortBytes(780 * 1024 * 1024), "780M");
            compare(QuickSettings.shortBytes(512), "512B");
            compare(QuickSettings.bytes(2400000000), "2.2 GB");
            compare(QuickSettings.bytes(310 * 1024 * 1024), "310 MB");
        }

        function test_recorders_are_app_streams_on_the_default_source() {
            const mic = {
                id: 50
            };
            const discord = {
                id: 90,
                isStream: true,
                properties: {
                    "application.name": "Discord"
                }
            };
            const loopback = {
                id: 91,
                isStream: false,
                name: "loopback"
            };
            const groups = [
                {
                    source: mic,
                    target: discord
                },
                {
                    source: mic,
                    target: discord
                },
                {
                    source: mic,
                    target: loopback
                },
                {
                    source: {
                        id: 51
                    },
                    target: {
                        id: 92,
                        isStream: true,
                        name: "obs"
                    }
                }
            ];
            compare(QuickSettings.recorders(mic, groups), ["Discord"]);
            compare(QuickSettings.recorders(null, groups), []);
            compare(QuickSettings.recordingText(["Discord"]), "Discord is using the mic");
            compare(QuickSettings.recordingText(["Discord", "OBS"]), "Discord and OBS are using the mic");
            compare(QuickSettings.recordingText([]), "");
        }

        function test_an_app_with_several_streams_is_one_entry() {
            const groups = QuickSettings.appGroups([
                {
                    id: 9,
                    name: "Firefox",
                    volume: 0.7,
                    muted: false
                },
                {
                    id: 12,
                    name: "Discord",
                    volume: 1,
                    muted: true
                },
                {
                    id: 10,
                    name: "Firefox",
                    volume: 0.4,
                    muted: true
                }
            ]);
            compare(groups.length, 2);
            compare(groups[0].name, "Firefox");
            compare(groups[0].id, 9);
            compare(groups[0].ids, [9, 10]);
            compare(groups[0].volume, 0.7);
            verify(!groups[0].muted, "one Firefox stream still plays");
            verify(groups[1].muted);

            const starting = QuickSettings.appGroups([
                {
                    id: 3,
                    name: "Spotify",
                    volume: 0,
                    muted: false,
                    ready: false
                },
                {
                    id: 4,
                    name: "Spotify",
                    volume: 0.6,
                    muted: false,
                    ready: true
                }
            ]);
            compare(starting[0].volume, 0.6, "a stream without a volume yet doesn't pull it to 0");

            const odd = QuickSettings.appGroups([
                {
                    id: 5,
                    name: "constructor",
                    volume: 0.5,
                    muted: false
                }
            ]);
            compare(odd[0].ids, [5], "any name an app gives is its own group");
        }

        function test_a_dragged_slider_follows_the_pointer_until_released() {
            const slider = createTemporaryObject(sliderComponent, stage, {
                value: 0.2
            });
            waitForRendering(slider);
            spy.signalName = "moved";
            spy.target = slider;
            const at = value => slider.trackX + value * slider.track;
            mousePress(slider, at(0.5), slider.height / 2);
            mouseMove(slider, at(0.8), slider.height / 2);
            // The volume set hasn't come back yet: it still shows the drag.
            compare(slider.current, 0.8);
            compare(spy.signalArguments[spy.count - 1][0], 0.8);
            mouseRelease(slider, at(0.8), slider.height / 2);
            compare(slider.current, 0.2, "released, it shows the volume again");
            mouseClick(slider, slider.trackX + 1, slider.height / 2);
            compare(spy.signalArguments[spy.count - 1][0], 0, "the track's start is 0");
            // The volume came back somewhere else meanwhile: clicking the
            // same spot again still sets it.
            slider.value = 0.7;
            const count = spy.count;
            mouseClick(slider, slider.trackX + 1, slider.height / 2);
            compare(spy.count, count + 1, "every press sets it");
        }

        function test_at_zero_the_track_is_empty_and_the_icon_beside_it() {
            const slider = createTemporaryObject(sliderComponent, stage, {
                value: 0,
                glyph: "volume_off"
            });
            waitForRendering(slider);
            const icon = findChild(slider, "icon");
            verify(icon.x + icon.width <= slider.trackX, "the icon isn't on the track");
            compare(findChild(slider, "fill").width, 0);
            spy.signalName = "iconClicked";
            spy.target = slider;
            mouseClick(icon);
            compare(spy.count, 1, "the icon mutes");
            slider.value = 0.5;
            slider.muted = true;
            compare(findChild(slider, "fill").width, slider.track / 2, "muted keeps the level");
            shot(slider, "slider-zero");
        }

        function test_a_long_greeting_shrinks_then_cuts_the_name() {
            const header = createTemporaryObject(headerComponent, stage, {
                width: 220,
                greeting: "Good afternoon",
                name: "Katov"
            });
            waitForRendering(header);
            const greeting = findChild(header, "greeting");
            const name = findChild(header, "name");
            verify(greeting.font.pixelSize < Tokens.fontSizeGreeting, "smaller to fit");
            verify(greeting.width + name.width <= header.width - name.parent.x + 1);
            compare(name.text, "Katov");
            header.name = "Maximiliana-Bartholomea";
            waitForRendering(header);
            compare(greeting.font.pixelSize, Tokens.fontSizeGreetingMin);
            verify(name.truncated, "a name too long is cut short");
            header.name = "";
            header.width = 400;
            compare(greeting.font.pixelSize, Tokens.fontSizeGreeting);
        }

        function test_graph_ends_at_the_right_edge() {
            compare(QuickSettings.graphPath([], 100, 50, 3, false), "");
            compare(QuickSettings.graphPath([0, 100], 100, 50, 3, false), "M50.0 50.0L100.0 0.0");
            compare(QuickSettings.graphPath([50], 100, 50, 3, true), "M100.0 25.0L100 50L100.0 50Z");
        }

        function test_main_page_shows_and_acts() {
            const page = createTemporaryObject(mainComponent, stage, {
                host: {
                    host: "RyzenShine",
                    user: "Luna",
                    booted_at: Date.now() / 1000 - 3 * 3600,
                    os: {
                        id: "nixos",
                        name: "NixOS"
                    }
                },
                outputAvailable: true,
                outputName: "Arctis Nova 5",
                outputKind: "headphones",
                volume: 0.64,
                micAvailable: true,
                recorders: ["Discord"],
                micVolume: 0.8,
                reading: reading()
            });
            waitForRendering(page);
            shot(page, "main");

            spy.signalName = "keepAwakeToggled";
            spy.target = page;
            mouseClick(page, 60, 80);
            compare(spy.count, 1, "the first tile is keep awake");

            spy.signalName = "pageRequested";
            spy.clear();
            mouseClick(page, page.width / 2, page.height - 40);
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], "resources");
        }

        function test_a_long_tray_scrolls_above_the_resources_card() {
            const apps = [];
            for (let i = 0; i < 64; i++)
                apps.push({
                    key: `app${i}.desktop`,
                    name: `App ${i}`,
                    icon: "",
                    appIcon: "",
                    attention: false,
                    hidden: false
                });
            const page = createTemporaryObject(mainComponent, stage, {
                trayApps: apps,
                reading: reading()
            });
            const scroll = findChild(page, "trayScroll");
            compare(scroll.height, Tokens.trayGridMaxRows * (Tokens.trayGridCell + Tokens.trayGridGap) - Tokens.trayGridGap);
            verify(scroll.contentHeight > scroll.height, "the rest scrolls");
        }

        function test_sound_page_switches_outputs() {
            const page = createTemporaryObject(soundComponent, stage, {
                outputAvailable: true,
                outputName: "Arctis Nova 5",
                outputKind: "headphones",
                volume: 0.64,
                outputs: [
                    {
                        id: 1,
                        name: "Arctis Nova 5",
                        kind: "headphones",
                        current: true
                    },
                    {
                        id: 2,
                        name: "GB206 HD Audio",
                        kind: "display",
                        current: false
                    }
                ],
                apps: QuickSettings.appGroups([
                    {
                        id: 9,
                        name: "Firefox",
                        volume: 0.72,
                        muted: false
                    },
                    {
                        id: 11,
                        name: "Firefox",
                        volume: 0.72,
                        muted: false
                    }
                ])
            });
            waitForRendering(page);
            compare(page.tab, "apps", "apps come first");
            page.tab = "output";
            waitForRendering(page);
            shot(page, "sound");
            spy.signalName = "outputPicked";
            spy.target = page;
            // The second row, under the header, the output card and the tabs.
            mouseClick(page, page.width / 2, 285);
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], 2);
            page.tab = "input";
            page.inputAvailable = true;
            page.inputName = "Arctis Nova 5 Mic";
            page.inputVolume = 0.8;
            waitForRendering(page);
            shot(page, "sound-input");
            spy.clear();
            spy.signalName = "inputVolumeMoved";
            // The card's slider sets the input's volume on this tab.
            mouseClick(page, 40 + (page.width - 2 * Tokens.pad - 40) / 2, 130);
            compare(spy.count, 1);
            page.tab = "apps";
            waitForRendering(page);
            shot(page, "sound-apps");
            spy.clear();
            spy.signalName = "appVolumeMoved";
            // The first app's slider, where the output list was.
            mouseClick(page, page.width / 2, 235);
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], [9, 11]);
        }

        function test_resources_page_lists_processes() {
            const page = createTemporaryObject(resourcesComponent, stage, {
                reading: reading()
            });
            waitForRendering(page);
            compare(page.memoryShare.toFixed(0), "29");
            spy.signalName = "endRequested";
            spy.target = page;
            const firefox = findChild(page, "app-/usr/lib/firefox/firefox");
            verify(firefox !== null);
            const x = firefox.mapToItem(page, firefox.width - Tokens.segmentPad - Tokens.endButtonWidth / 2, firefox.height / 2);
            mouseMove(page, x.x, x.y);
            mouseClick(page, x.x, x.y);
            compare(spy.count, 0, "✕ only asks");
            compare(page.asked, "/usr/lib/firefox/firefox");
            const confirm = findChild(page, "confirm-/usr/lib/firefox/firefox");
            verify(confirm.visible);
            waitForRendering(page);
            shot(page, "resources");
            mouseClick(findChild(confirm, "endConfirm"));
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], "/usr/lib/firefox/firefox");
            compare(page.asked, "");

            page.query = "NIR";
            compare(page.shownApps.length, 1, "the search matches any case");
            compare(page.shownApps[0].name, "niri");
        }

        function test_more_apps_than_fit_scroll() {
            const many = reading();
            many.apps = Array.from({
                length: 25
            }, (_, i) => ({
                        name: `app${i}`,
                        key: `/usr/bin/app${i}`,
                        processes: 1,
                        cpu: 0,
                        memory: (25 - i) * 100000000,
                        protected: i === 24
                    }));
            const page = createTemporaryObject(resourcesComponent, stage, {
                reading: many
            });
            waitForRendering(page);
            const list = findChild(page, "apps");
            const bottom = list.mapToItem(page, 0, list.height).y;
            verify(Math.abs(bottom - page.height) < 1, "the list fills the page");
            verify(list.contentHeight > list.height, "more than fit");
            verify(findChild(page, "app-/usr/bin/app24") !== null, "the last is there to scroll to");
            shot(page, "resources-many");
        }
    }
}
