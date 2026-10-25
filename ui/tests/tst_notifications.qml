import QtQuick
import QtTest
import "../bar"
import "../notifications"
import "../quick_settings"
import "../theme"
import "../notifications/notifications.js" as Notifications

// Notifications' logic, the pop-ups' stack, quick settings' list and
// settings page, and the badge, driven with notifications in the shape
// novad sends them. The root is an Item: a TestCase root gets no mouse
// events.
Item {
    id: stage
    width: 420
    height: 900

    TestCase {
        name: "Notifications"
        when: windowShown

        Component {
            id: stackComponent
            NotificationStack {}
        }
        Component {
            id: cardComponent
            NotificationCard {}
        }
        Component {
            id: listComponent
            NotificationList {
                width: 336
                height: 520
            }
        }
        Component {
            id: settingsComponent
            NotificationSettingsPage {
                width: 336
                height: 600
            }
        }
        Component {
            id: buttonsComponent
            PillButtons {
                quickSettings: true
                power: true
            }
        }

        SignalSpy {
            id: spy
        }

        function init() {
            Tokens.apply({
                bg: "#1D1D1D",
                fg: "#F5E7C6",
                fgAlt: "#D8CCB0",
                fgDim: "#9A9A9A",
                primary: "#FD7014",
                secondary: "#BE95FF",
                onAccent: "#1D1D1D",
                error: "#EE5396"
            });
            spy.target = null;
            spy.signalName = "";
            spy.clear();
        }

        // A notification as novad sends it.
        function note(id, app, fields) {
            return Object.assign({
                id: id,
                app: app,
                summary: `${app} ${id}`,
                body: "",
                urgency: "normal",
                time: 1761400000,
                live: true
            }, fields ?? {});
        }

        // Pop-up `id`'s card, once it has grown into place.
        function settled(stack, id) {
            tryVerify(() => findChild(stack, `card-${id}`) !== null);
            const card = findChild(stack, `card-${id}`);
            tryCompare(card.parent, "presence", 1);
            return card;
        }

        function test_items_group_by_app_newest_first() {
            const groups = Notifications.groups([note(5, "Discord"), note(4, "Ghostty"), note(3, "Discord"), note(1, "Firefox")]);
            compare(groups.map(group => group.app), ["Discord", "Ghostty", "Firefox"]);
            compare(groups[0].items.map(item => item.id), [5, 3]);
        }

        function test_times_read_short() {
            compare(Notifications.ago(1000, 1030), "now");
            compare(Notifications.ago(1000, 1000 + 5 * 60), "5m");
            compare(Notifications.ago(1000, 1000 + 2 * 3600 + 10), "2h");
            compare(Notifications.ago(1000, 1000 + 3 * 86400), "3d");
            compare(Notifications.ago(1000, 900), "now", "a clock that went back");
        }

        function test_timeouts_follow_the_app_except_critical_and_running_progress() {
            compare(Notifications.timeout(note(1, "A"), 6000), 6000);
            compare(Notifications.timeout(note(1, "A", {
                timeout_ms: 3000
            }), 6000), 3000);
            compare(Notifications.timeout(note(1, "A", {
                timeout_ms: 0
            }), 6000), 0);
            compare(Notifications.timeout(note(1, "A", {
                urgency: "critical",
                timeout_ms: 3000
            }), 6000), 0);
            compare(Notifications.timeout(note(1, "A", {
                progress: 40
            }), 6000), 0);
            compare(Notifications.timeout(note(1, "A", {
                progress: 100
            }), 6000), 6000);
        }

        function test_a_click_goes_to_the_app_one_way_or_another() {
            compare(Notifications.clickDoes(note(1, "A", {
                has_default: true
            })), "action");
            compare(Notifications.clickDoes(note(1, "A", {
                has_default: true,
                live: false,
                app_id: "a.desktop"
            })), "launch", "kept from before novad restarted");
            compare(Notifications.clickDoes(note(1, "A", {
                app_id: "a.desktop"
            })), "launch");
            compare(Notifications.clickDoes(note(1, "A")), "none");
        }

        function test_a_newer_one_from_the_same_app_takes_a_plain_pop_ups_place() {
            const stack = createTemporaryObject(stackComponent, stage);
            stack.add(note(1, "Discord"));
            stack.add(note(2, "Ghostty"));
            stack.add(note(3, "Discord"));
            const shown = stack.shownEntries();
            compare(shown.map(entry => entry.notification.id), [2, 3], "Discord's pop-up shows the newest, in place");
            compare(shown[1].count, 2);
            tryVerify(() => findChild(stack, "card-3") !== null);
            verify(findChild(findChild(stack, "card-3"), "more").visible, "+1");

            // Progress updates its own pop-up; critical ones stay their own.
            stack.add(note(4, "Files", {
                progress: 10
            }));
            stack.add(note(4, "Files", {
                progress: 60
            }));
            stack.add(note(5, "Discord", {
                urgency: "critical"
            }));
            compare(stack.shownEntries().map(entry => entry.notification.id), [5, 4, 2]);
            compare(stack.shownEntries()[1].notification.progress, 60);
            compare(stack.shownEntries()[1].count, 1);
            compare(stack.waiting.map(entry => entry.notification.id), [3], "the oldest waits for room");
            tryVerify(() => findChild(stack, "card-3") === null, 2000, "its card folds away meanwhile");
        }

        function test_more_than_three_wait_and_come_up_as_room_frees() {
            const stack = createTemporaryObject(stackComponent, stage);
            for (const [id, app] of [[1, "A"], [2, "B"], [3, "C"], [4, "D"], [5, "E"]])
                stack.add(note(id, app));
            compare(stack.shownEntries().map(entry => entry.notification.id), [5, 4, 3]);
            compare(stack.waiting.map(entry => entry.notification.id), [2, 1]);
            const card = settled(stack, 5);
            spy.target = stack;
            spy.signalName = "dismissed";
            mouseMove(card, card.width / 2, card.height / 2);
            const dismiss = findChild(card, "dismiss");
            tryVerify(() => dismiss.visible);
            mouseClick(dismiss);
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], 5);
            compare(stack.shownEntries().map(entry => entry.notification.id), [4, 3, 2], "the newest waiting one comes up");
            compare(stack.waiting.map(entry => entry.notification.id), [1]);
        }

        function test_a_pop_up_closed_elsewhere_goes_and_clear_takes_them_all() {
            const stack = createTemporaryObject(stackComponent, stage);
            stack.add(note(1, "A"));
            stack.add(note(2, "B"));
            stack.sync([note(1, "A"), note(2, "B")]);
            stack.sync([note(2, "B")]);
            compare(stack.shownEntries().map(entry => entry.notification.id), [2]);
            // Transient ones are never listed, so syncing leaves them.
            stack.add(note(3, "Volume"));
            stack.sync([note(2, "B")]);
            compare(stack.shownEntries().map(entry => entry.notification.id), [3, 2]);
            // Each shows its notification as the list has it now.
            stack.sync([note(2, "B", {
                    live: false
                }), note(3, "Volume")]);
            compare(stack.shownEntries()[1].notification.live, false, "its app quit");
            stack.sync([note(2, "B")]);
            compare(stack.shownEntries().map(entry => entry.notification.id), [2], "a transient one closed by its app goes");
            stack.clear();
            compare(stack.shownEntries().length, 0);
            tryVerify(() => !stack.busy, 2000, "the cards fold away");
        }

        function test_a_pop_up_runs_out_and_the_pointer_holds_it() {
            const stack = createTemporaryObject(stackComponent, stage);
            stack.add(note(1, "A", {
                timeout_ms: 300
            }));
            const card = settled(stack, 1);
            mouseMove(card, card.width / 2, card.height / 2);
            wait(500);
            compare(stack.shownEntries().length, 1, "held while the pointer is on it");
            mouseMove(stage, stage.width - 1, stage.height - 1);
            tryVerify(() => stack.shownEntries().length === 0, 2000, "it runs out once the pointer leaves");
        }

        function test_a_card_shows_its_buttons_and_reports_clicks() {
            const card = createTemporaryObject(cardComponent, stage, {
                notification: note(7, "Firefox", {
                    body: "niri.tar.gz",
                    actions: [
                        {
                            key: "open",
                            label: "Open file"
                        },
                        {
                            key: "show",
                            label: "Show in folder"
                        }
                    ],
                    urgency: "critical"
                })
            });
            verify(!findChild(card, "timer").visible, "critical ones stay");
            spy.target = card;
            spy.signalName = "actionClicked";
            const label = findChild(card, "dismiss");
            verify(label);
            // The second button.
            const buttons = [];
            const walk = item => {
                for (const child of item.children) {
                    if (child.modelData?.key)
                        buttons.push(child);
                    walk(child);
                }
            };
            walk(card);
            compare(buttons.length, 2);
            mouseClick(buttons[1]);
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], "show");
        }

        function test_buttons_that_dont_fit_wrap_and_stay_on_the_card() {
            const actions = [];
            for (let i = 0; i < 8; i++)
                actions.push({
                    key: `a${i}`,
                    label: `A rather long label for button number ${i} that goes on`
                });
            const card = createTemporaryObject(cardComponent, stage, {
                notification: note(8, "Chat", {
                    actions: actions
                })
            });
            const buttons = [];
            const walk = item => {
                for (const child of item.children) {
                    if (child.modelData?.key)
                        buttons.push(child);
                    walk(child);
                }
            };
            walk(card);
            compare(buttons.length, 8);
            const onCard = button => {
                const at = button.mapToItem(card, 0, 0);
                return at.x >= 0 && at.x + button.width <= card.width && at.y + button.height <= card.height;
            };
            // Once the buttons are laid out.
            tryVerify(() => buttons.every(onCard), 1000, "every button fits on the card");
            verify(buttons[7].mapToItem(card, 0, 0).y > buttons[0].mapToItem(card, 0, 0).y, "they wrap");
        }

        function test_the_list_stacks_apps_and_opens_a_stack_in_place() {
            const list = createTemporaryObject(listComponent, stage, {
                items: [note(5, "Discord", {
                        unread: true
                    }), note(4, "Ghostty"), note(3, "Discord"), note(2, "Discord")]
            });
            verify(!findChild(list, "empty").visible);
            verify(findChild(list, "row-5") !== null);
            compare(findChild(list, "row-3"), null, "the stack shows its newest only");
            mouseClick(findChild(list, "row-5"));
            tryVerify(() => findChild(list, "row-3") !== null, 1000, "a click opens the stack");

            spy.target = list;
            spy.signalName = "cleared";
            mouseClick(findChild(list, "clearApp"));
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], "Discord");

            spy.clear();
            mouseClick(findChild(list, "clearAll"));
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], null);
        }

        function test_a_row_opens_or_mutes_its_app_or_goes() {
            const list = createTemporaryObject(listComponent, stage, {
                items: [note(4, "Ghostty"), note(1, "Firefox")]
            });
            spy.target = list;
            spy.signalName = "opened";
            mouseClick(findChild(list, "row-4"));
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0].id, 4);

            const row = findChild(list, "row-1");
            mouseMove(row, row.width / 2, row.height / 2);
            const more = findChild(row, "more");
            tryVerify(() => more.visible);
            mouseClick(more);
            verify(findChild(row, "menu").visible);
            spy.clear();
            spy.signalName = "muted";
            mouseClick(findChild(row, "mute"));
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], "Firefox");
            verify(!findChild(row, "menu").visible, "picking closes the menu");

            mouseClick(more);
            spy.clear();
            spy.signalName = "dismissed";
            mouseClick(findChild(row, "dismiss"));
            compare(spy.signalArguments[0][0], 1);
        }

        function test_an_empty_list_is_all_caught_up() {
            const list = createTemporaryObject(listComponent, stage, {
                items: []
            });
            verify(findChild(list, "empty").visible);
            verify(!findChild(list, "clearAll").visible);
            spy.target = list;
            spy.signalName = "doNotDisturbToggled";
            mouseClick(findChild(list, "doNotDisturb"));
            compare(spy.count, 1);
        }

        function test_settings_mute_and_unmute_apps() {
            const page = createTemporaryObject(settingsComponent, stage, {
                apps: [
                    {
                        name: "Discord"
                    },
                    {
                        name: "Firefox",
                        icon: "firefox"
                    }
                ],
                muted: ["Discord"]
            });
            spy.target = page;
            spy.signalName = "muteToggled";
            const rows = [];
            const walk = item => {
                for (const child of item.children) {
                    if (child.modelData?.name)
                        rows.push(child);
                    walk(child);
                }
            };
            walk(page);
            compare(rows.map(row => row.subtitle), ["Muted: list only, no pop-ups", "Pop-ups and list"]);
            mouseClick(rows[0]);
            compare([...spy.signalArguments[0]], ["Discord", false]);
            mouseClick(rows[1]);
            compare([...spy.signalArguments[1]], ["Firefox", true]);
        }

        function test_the_badge_counts_unread_or_shows_do_not_disturb() {
            const buttons = createTemporaryObject(buttonsComponent, stage);
            const badge = findChild(buttons, "badge");
            verify(!badge.shown);
            buttons.unread = 3;
            verify(badge.shown);
            buttons.unread = 12;
            buttons.quickSettingsOpen = true;
            verify(!badge.shown, "none while quick settings is open");
            buttons.quickSettingsOpen = false;
            buttons.unread = 0;
            buttons.doNotDisturb = true;
            verify(badge.shown);
            compare(badge.color, Tokens.badgeQuiet);
        }
    }
}
