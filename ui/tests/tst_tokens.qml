import QtQuick
import QtTest
import "../theme"

TestCase {
    name: "Tokens"

    function test_radii_shrink_from_panel_to_control() {
        verify(Tokens.radiusPanel > Tokens.radiusCard);
        verify(Tokens.radiusCard > Tokens.radiusControl);
    }

    function test_panel_motion_is_250ms() {
        compare(Tokens.durationPanel, 250);
    }

    function test_apply_sets_colors_and_glass_follows_them() {
        Tokens.apply({
            bg: "#1D1D1D",
            fg: "#F5E7C6",
            primary: "#FD7014"
        });
        verify(Qt.colorEqual(Tokens.primary, "#FD7014"));
        verify(Qt.colorEqual(Tokens.fg, "#F5E7C6"));
        verify(Qt.colorEqual(Qt.rgba(Tokens.surface.r, Tokens.surface.g, Tokens.surface.b, 1), "#1D1D1D"));
        compare(Tokens.surface.a, Tokens.surfaceOpacity);
        verify(Tokens.surface.a > 0 && Tokens.surface.a < 1);
        verify(Qt.colorEqual(Qt.rgba(Tokens.outline.r, Tokens.outline.g, Tokens.outline.b, 1), "#F5E7C6"));
    }

    function test_tint_runs_from_bg_to_the_color() {
        Tokens.apply({
            bg: "#000000",
            primary: "#FF8000"
        });
        verify(Qt.colorEqual(Tokens.tint(Tokens.primary, 0), "#000000"));
        verify(Qt.colorEqual(Tokens.tint(Tokens.primary, 1), "#FF8000"));
        const half = Tokens.tint(Tokens.primary, 0.5);
        fuzzyCompare(half.r, 0.5, 0.01);
        fuzzyCompare(half.g, 0.25, 0.01);
        compare(half.a, 1);
    }

    function test_apply_warns_about_unknown_tokens() {
        ignoreWarning(/theme token sparkle has no property/);
        Tokens.apply({
            sparkle: "#FFFFFF"
        });
        verify(!("sparkle" in Tokens));
    }
}
