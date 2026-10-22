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

    function test_apply_warns_about_unknown_tokens() {
        ignoreWarning(/theme token sparkle has no property/);
        Tokens.apply({
            sparkle: "#FFFFFF"
        });
        verify(!("sparkle" in Tokens));
    }
}
