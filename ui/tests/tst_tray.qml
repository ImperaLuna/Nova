import QtQuick
import QtTest
import "../bar"
import "../theme"
import "../tray"
import "../tray/tray.js" as Tray

// The tray: joining Quickshell's items with novad's tray event, what the
// bar shows and in which order, and the right pill growing its strip of
// icons behind the chevron. Items are plain objects in the shape
// Quickshell's SystemTrayItem has. The root is an Item: a TestCase root
// gets no mouse events.
Item {
    id: stage
    width: 800
    height: 200

    // Recorded on RyzenShine (2026-10-25): Electron sends an empty title
    // and an id that doesn't name the app; Steam names itself.
    function quickshellItems() {
        return [
            {
                id: "discord_status_icon_1",
                title: "",
                status: 1,
                tooltipTitle: "",
                icon: "image://qsimage/discord"
            },
            {
                id: "steam",
                title: "Steam",
                status: 1,
                tooltipTitle: "Steam",
                icon: "image://icon/steam_tray_mono"
            },
            {
                id: "chrome_status_icon_1",
                title: "",
                status: Tray.STATUS_NEEDS_ATTENTION,
                tooltipTitle: "Claude",
                icon: "image://qsimage/claude"
            }
        ];
    }
    function novadTray() {
        return {
            items: [
                {
                    address: ":1.421/StatusNotifierItem",
                    id: "discord_status_icon_1",
                    title: "",
                    app: {
                        id: "discord.desktop",
                        name: "Discord",
                        icon: "discord"
                    }
                },
                {
                    address: ":1.607/org/ayatana/NotificationItem/steam",
                    id: "steam",
                    title: "Steam"
                },
                {
                    address: ":1.433/StatusNotifierItem",
                    id: "chrome_status_icon_1",
                    title: "",
                    app: {
                        id: "com.anthropic.Claude.desktop",
                        name: "Claude"
                    }
                }
            ],
            hidden: ["steam"]
        };
    }
    // An entry for the bar, as TrayService gives them.
    function entry(key: string, attention: bool): var {
        return {
            item: {},
            key: key,
            name: key,
            icon: "",
            appIcon: "",
            attention: attention,
            hidden: false
        };
    }

    TestCase {
        name: "Tray"
        when: windowShown

        Component {
            id: pillComponent
            RightPill {
                quickSettings: true
                power: true
            }
        }

        Component {
            id: menuComponent
            TrayMenuList {}
        }

        SignalSpy {
            id: spy
        }

        // Where shot() saves what a menu shows, to look at it; set it by
        // hand while working on the tray.
        property string shotDir: ""
        function shot(item, file) {
            const image = grabImage(item);
            verify(image.width > 0);
            if (shotDir !== "")
                image.save(`${shotDir}/${file}`);
        }

        // Menu entries in the shape Quickshell's QsMenuEntry has; `ran`
        // counts the triggers.
        property var ran: []
        function menuEntry(text: string, extra: var): var {
            const entry = {
                text: text,
                isSeparator: false,
                enabled: true,
                buttonType: 0,
                checkState: Qt.Unchecked,
                hasChildren: false,
                triggered: () => ran.push(text)
            };
            return Object.assign(entry, extra ?? {});
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

        function test_items_are_matched_to_their_apps() {
            const all = Tray.entries(stage.quickshellItems(), stage.novadTray());
            compare(all.map(entry => entry.key), ["discord.desktop", "steam", "com.anthropic.Claude.desktop"]);
            compare(all.map(entry => entry.name), ["Discord", "Steam", "Claude"], "Electron's empty titles get the app's name");
            compare(all[0].appIcon, "discord");
            compare(all[1].appIcon, "", "no app: the item's own image");
            compare(all[1].icon, "image://icon/steam_tray_mono");
            compare(all.map(entry => entry.hidden), [false, true, false]);
            compare(all.map(entry => entry.attention), [false, false, true]);
        }

        function test_without_novad_items_still_show_under_their_own_names() {
            const all = Tray.entries(stage.quickshellItems(), null);
            compare(all.map(entry => entry.key), ["discord_status_icon_1", "steam", "chrome_status_icon_1"]);
            compare(all.map(entry => entry.name), ["discord_status_icon_1", "Steam", "Claude"]);
        }

        function test_a_changed_title_still_finds_the_app() {
            const items = stage.quickshellItems();
            items[0].title = "Discord (3)";
            compare(Tray.entries(items, stage.novadTray())[0].key, "discord.desktop");
        }

        function test_items_novad_leaves_out_are_not_shown() {
            const items = stage.quickshellItems();
            items.push({
                id: "steam",
                title: "Steam",
                status: 1,
                tooltipTitle: "",
                icon: ""
            });
            const all = Tray.entries(items, stage.novadTray());
            compare(all.map(entry => entry.key), ["discord.desktop", "steam", "com.anthropic.Claude.desktop"], "the X11 bridge's Steam stays out");
        }

        function test_two_apps_with_the_same_id_keep_their_own_apps() {
            const item = {
                id: "chrome_status_icon_1",
                title: "",
                status: 0,
                tooltipTitle: "",
                icon: ""
            };
            const app = (id, name) => ({
                        id: item.id,
                        title: "",
                        app: {
                            id: id,
                            name: name
                        }
                    });
            const all = Tray.entries([item, Object.assign({}, item)], {
                items: [app("a.desktop", "A"), app("b.desktop", "B")],
                hidden: ["a.desktop"]
            });
            compare(all.map(entry => entry.key), ["a.desktop", "b.desktop"]);
            compare(all.map(entry => entry.hidden), [true, false], "hiding one leaves the other");
        }

        function test_the_bar_shows_attention_first_and_hides_hidden_apps() {
            const shown = Tray.shown(Tray.entries(stage.quickshellItems(), stage.novadTray()));
            compare(shown.map(entry => entry.key), ["com.anthropic.Claude.desktop", "discord.desktop"]);
            verify(Tray.anyAttention(Tray.entries(stage.quickshellItems(), stage.novadTray())));
        }

        function test_menu_text() {
            compare(Tray.label("_Open Discord"), "Open Discord");
            compare(Tray.label("snake__case"), "snake_case");
            compare(Tray.label("Disable Notifications && Audio"), "Disable Notifications & Audio");
            verify(Tray.isQuit("Quit Discord"));
            verify(Tray.isQuit("Exit Steam"));
            verify(!Tray.isQuit("Quick Entry"));
            verify(!Tray.isQuit(""));
        }

        function test_more_counts_what_is_out_of_sight() {
            compare(Tray.moreCount(8, 5, 0), 3);
            compare(Tray.moreCount(8, 5, 2), 1);
            compare(Tray.moreCount(8, 5, 3), 0);
            compare(Tray.moreCount(3, 5, 0), 0);
        }

        function test_without_tray_apps_the_pill_is_as_before() {
            const pill = createTemporaryObject(pillComponent, stage);
            const chevron = findChild(pill, "chevron");
            verify(!chevron.visible);
            compare(pill.implicitWidth, 2 * Tokens.pillButtonInset + 2 * Tokens.pillButtonSize + Tokens.pillButtonGap);
        }

        function test_the_chevron_opens_up_to_five_icons_then_scrolls() {
            const apps = [];
            for (let i = 0; i < 8; i++)
                apps.push(stage.entry(`app${i}.desktop`, false));
            const pill = createTemporaryObject(pillComponent, stage, {
                trayApps: apps
            });
            const closed = pill.implicitWidth;
            const chevron = findChild(pill, "chevron");
            verify(chevron.visible);
            compare(closed, 2 * Tokens.pillButtonInset + 3 * Tokens.pillButtonSize + 3 * Tokens.pillButtonGap + Tokens.dividerWidth);

            mouseClick(chevron);
            verify(pill.open);
            tryCompare(pill, "moving", false);
            const strip = findChild(pill, "strip");
            compare(strip.width, 5 * Tokens.pillButtonSize + 4 * Tokens.pillButtonGap, "five icons show");
            compare(findChild(pill, "more").text, "+3");
            verify(pill.implicitWidth > closed + strip.width);

            mouseClick(chevron);
            verify(!pill.open);
            tryCompare(pill, "implicitWidth", closed);
        }

        function test_clicks_on_icons_say_which_app_and_where() {
            const pill = createTemporaryObject(pillComponent, stage, {
                trayApps: [stage.entry("discord.desktop", false), stage.entry("steam.desktop", true)]
            });
            pill.open = true;
            tryCompare(pill, "moving", false);
            spy.target = pill;
            spy.signalName = "trayMenuRequested";
            const steam = findChild(pill, "tray-steam.desktop");
            mouseClick(steam, steam.width / 2, steam.height / 2, Qt.RightButton);
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0].key, "steam.desktop");
            const at = steam.mapToItem(pill, steam.width / 2, steam.height / 2);
            compare(spy.signalArguments[0][1], at);
        }

        function test_a_menu_runs_entries_and_offers_to_hide_the_app() {
            ran = [];
            const discord = stage.entry("discord.desktop", false);
            discord.name = "Discord";
            const menu = createTemporaryObject(menuComponent, stage, {
                entry: discord,
                entries: [menuEntry("_Open Discord"), menuEntry("", {
                        isSeparator: true
                    }), menuEntry("Mute", {
                        buttonType: 1,
                        checkState: Qt.Checked
                    }), menuEntry("Deafen", {
                        buttonType: 1
                    }), menuEntry("Check for updates", {
                        enabled: false
                    }), menuEntry("", {
                        isSeparator: true
                    }), menuEntry("Quit Discord")]
            });
            shot(menu, "tray-menu.png");
            spy.target = menu;
            spy.signalName = "done";

            mouseClick(menu, menu.width / 2, menu.padding + Tokens.menuRowHeight * 1.5);
            compare(ran, ["_Open Discord"]);
            compare(spy.count, 1, "an entry that ran is done");

            const separator = 2 * Tokens.menuSeparatorGap + Tokens.dividerWidth;
            mouseClick(menu, menu.width / 2, menu.padding + Tokens.menuRowHeight * 2.5 + separator);
            compare(ran, ["_Open Discord", "Mute"]);
            compare(spy.count, 1, "a checkbox flips in place");

            mouseClick(menu, menu.width / 2, menu.padding + Tokens.menuRowHeight * 4.5 + separator);
            compare(ran.length, 2, "a disabled entry does nothing");

            const hides = createTemporaryObject(spyComponent, stage, {
                target: menu,
                signalName: "hideRequested"
            });
            mouseClick(menu, menu.width / 2, menu.height - menu.padding - Tokens.menuRowHeight / 2);
            compare(hides.count, 1);
            compare(hides.signalArguments[0][0], "discord.desktop");
            compare(hides.signalArguments[0][1], true);
        }

        Component {
            id: spyComponent
            SignalSpy {}
        }

        function test_the_tray_alone_has_no_divider() {
            const pill = createTemporaryObject(pillComponent, stage, {
                quickSettings: false,
                power: false,
                trayApps: [stage.entry("discord.desktop", false)]
            });
            verify(findChild(pill, "chevron").visible);
            compare(pill.implicitWidth, 2 * Tokens.pillButtonInset + Tokens.pillButtonSize);
        }

        function test_a_long_menu_scrolls_and_keeps_hide_in_reach() {
            const entries = [];
            for (let i = 0; i < 40; i++)
                entries.push(menuEntry(`Entry ${i}`));
            const maxHeight = 10 * Tokens.menuRowHeight;
            const menu = createTemporaryObject(menuComponent, stage, {
                entry: stage.entry("big.desktop", false),
                entries: entries,
                maxHeight: maxHeight
            });
            verify(menu.implicitHeight <= maxHeight, `${menu.implicitHeight} fits ${maxHeight}`);
            const hides = createTemporaryObject(spyComponent, stage, {
                target: menu,
                signalName: "hideRequested"
            });
            mouseClick(menu, menu.width / 2, menu.implicitHeight - menu.padding - Tokens.menuRowHeight / 2);
            compare(hides.count, 1, "Hide from tray is still on screen");
        }

        function test_app_text_shows_as_written() {
            const app = stage.entry("odd.desktop", false);
            app.name = "<b>Bold</b> app";
            const menu = createTemporaryObject(menuComponent, stage, {
                entry: app,
                entries: [menuEntry("<i>Italic</i>")]
            });
            const texts = [];
            const walk = item => {
                for (const child of item.children) {
                    if (child instanceof Text && child.visible)
                        texts.push(child);
                    walk(child);
                }
            };
            walk(menu);
            const shown = texts.filter(text => text.text.indexOf("<") !== -1);
            compare(shown.length, 2);
            for (const text of shown)
                compare(text.textFormat, Text.PlainText, text.text);
        }

        function test_losing_every_app_closes_the_tray() {
            const pill = createTemporaryObject(pillComponent, stage, {
                trayApps: [stage.entry("discord.desktop", false)]
            });
            pill.open = true;
            pill.trayApps = [];
            verify(!pill.open);
        }
    }
}
