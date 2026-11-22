import QtQuick
import QtTest
import "../ai_usage/usage.js" as Usage

// Events in the shape novad sends (see the ai_usage tests in app/).
TestCase {
    name: "AiUsage"

    readonly property date now: new Date("2026-10-23T08:00:00Z")

    function event() {
        return {
            shown: "codex",
            providers: [
                {
                    provider: "claude",
                    state: "ready",
                    limits: [
                        {
                            window: "session",
                            percent: 42,
                            resets_at: "2026-10-23T10:14:00Z"
                        },
                        {
                            window: "weekly",
                            percent: 61,
                            resets_at: "2026-10-27T03:00:00Z"
                        }
                    ]
                },
                {
                    provider: "codex",
                    state: "ready",
                    plan: "Pro",
                    limits: [
                        {
                            window: "weekly",
                            percent: 84,
                            resets_at: "2026-10-29T10:20:00Z"
                        }
                    ]
                }
            ]
        };
    }

    function test_session_comes_first_and_weekly_second() {
        const claude = Usage.summarize(event().providers[0], now);
        compare(claude.name, "Claude");
        compare(claude.hero, "42");
        compare(claude.main.name, "5-hour session");
        compare(claude.main.left, "2h 14m left");
        compare(claude.second.name, "Weekly");
        compare(claude.plan, "");
        verify(!claude.warn);
    }

    function test_a_weekly_only_plan_shows_its_weekly_limit() {
        const codex = Usage.summarize(event().providers[1], now);
        compare(codex.hero, "84");
        compare(codex.main.name, "Weekly");
        compare(codex.second, null);
        compare(codex.plan, "Pro");
        verify(codex.warn, "84% is past the warning line");
    }

    function test_warning_starts_at_80() {
        verify(!Usage.isWarn(79));
        verify(Usage.isWarn(80));
    }

    function test_reset_text_is_relative_within_a_day() {
        compare(Usage.resetText("2026-10-23T08:45:00Z", now), "Resets in 45m");
        compare(Usage.resetText("2026-10-23T11:00:00Z", now), "Resets in 3h");
        const later = new Date("2026-10-27T03:00:00Z");
        const expected = "Resets " + ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][later.getDay()] + " " + ("0" + later.getHours()).slice(-2) + ":" + ("0" + later.getMinutes()).slice(-2);
        compare(Usage.resetText("2026-10-27T03:00:00Z", now), expected);
        compare(Usage.resetText("2026-10-23T07:00:00Z", now), "", "a past reset says nothing");
        compare(Usage.resetText(undefined, now), "");
    }

    function test_durations_round_up() {
        compare(Usage.duration(30 * 1000), "1m");
        compare(Usage.duration(2 * 3600 * 1000), "2h");
        compare(Usage.duration((3 * 24 + 2) * 3600 * 1000), "3d 2h");
    }

    function test_unavailable_and_loading_have_no_number() {
        const off = Usage.summarize({
            provider: "claude",
            state: "unavailable",
            hint: "Set nova claude-statusline"
        }, now);
        compare(off.hero, "–");
        compare(off.main, null);
        compare(off.message, "Set nova claude-statusline");
        const loading = Usage.summarize({
            provider: "codex",
            state: "loading"
        }, now);
        compare(loading.hero, "…");
    }

    function test_shown_and_others_split_the_providers() {
        compare(Usage.shown(event()).provider, "codex");
        compare(Usage.others(event()).map(p => p.provider), ["claude"]);
        compare(Usage.shown(null), null);
    }
}
