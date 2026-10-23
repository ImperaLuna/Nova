import QtQuick
import QtTest
import "../ai_usage"
import "../theme"

// The real pill and panel, driven with events in the shape novad sends.
// The root is an Item: a TestCase root gets no mouse events.
Item {
    id: stage
    width: 400
    height: 400

    TestCase {
        id: test
        name: "AiUsagePill"
        when: windowShown

        Component {
            id: pillComponent
            AiUsage {}
        }

        SignalSpy {
            id: showSpy
            signalName: "showRequested"
        }

        SignalSpy {
            id: refreshSpy
            signalName: "refreshRequested"
        }

        function usage(shown) {
            const soon = new Date(Date.now() + 2 * 3600 * 1000).toISOString();
            return {
                shown: shown,
                providers: [
                    {
                        provider: "claude",
                        state: "ready",
                        limits: [
                            {
                                window: "session",
                                percent: 42,
                                resets_at: soon
                            },
                            {
                                window: "weekly",
                                percent: 61
                            }
                        ]
                    },
                    {
                        provider: "codex",
                        state: "ready",
                        plan: "Plus",
                        limits: [
                            {
                                window: "weekly",
                                percent: 84
                            }
                        ]
                    }
                ]
            };
        }

        function init() {
            Tokens.apply({
                bg: "#1D1D1D",
                fg: "#F5E7C6",
                fgDim: "#9A9A9A",
                primary: "#FD7014"
            });
        }

        function createPill(shown) {
            const pill = createTemporaryObject(pillComponent, stage, {
                usage: usage(shown)
            });
            showSpy.target = pill;
            showSpy.clear();
            refreshSpy.target = pill;
            refreshSpy.clear();
            return pill;
        }

        function test_closed_pill_is_small_and_one_bar_tall() {
            const pill = createPill("claude");
            compare(pill.height, Tokens.barHeight);
            verify(pill.width < 100, `closed width ${pill.width}`);
            compare(pill.summary.hero, "42");
        }

        function test_clicking_the_pill_opens_it_in_place_and_again_closes_it() {
            const pill = createPill("claude");
            const closedWidth = pill.width;
            mouseClick(pill, 20, Tokens.barHeight / 2);
            tryCompare(pill, "t", 1);
            compare(pill.width, pill.panelWidth);
            compare(pill.height, pill.openHeight);
            verify(pill.openHeight > 200, `open height ${pill.openHeight}`);
            mouseClick(pill, 20, Tokens.barHeight / 2);
            tryCompare(pill, "t", 0);
            compare(pill.width, closedWidth);
        }

        function test_opening_asks_for_fresh_numbers_and_closing_does_not() {
            const pill = createPill("claude");
            mouseClick(pill, 20, Tokens.barHeight / 2);
            compare(refreshSpy.count, 1);
            mouseClick(pill, 20, Tokens.barHeight / 2);
            compare(refreshSpy.count, 1);
        }

        function test_warn_color_starts_at_80() {
            const pill = createPill("codex");
            verify(pill.summary.warn);
            verify(Qt.colorEqual(pill.percentColor(pill.summary), Tokens.primary));
            const claude = createPill("claude");
            verify(Qt.colorEqual(claude.percentColor(claude.summary), Tokens.fg));
        }

        function test_clicking_another_provider_asks_to_show_it() {
            const pill = createPill("claude");
            pill.open = true;
            tryCompare(pill, "t", 1);
            // The only other provider's row is the panel's last 40px, above the bottom padding.
            mouseClick(pill, pill.panelWidth / 2, pill.openHeight - pill.panelBottom - pill.rowHeight / 2);
            compare(showSpy.count, 1);
            compare(showSpy.signalArguments[0][0], "codex");
        }

        function test_weekly_only_panel_is_shorter_than_session_and_weekly() {
            const claude = createPill("claude");
            const codex = createPill("codex");
            verify(codex.openHeight < claude.openHeight);
        }
    }
}
