import QtQuick
import QtTest
import "../theme"

TestCase {
    name: "Tokens"

    function test_radii_shrink_from_panel_to_control() {
        verify(Tokens.radiusPanel > Tokens.radiusCard);
        verify(Tokens.radiusCard > Tokens.radiusControl);
    }

    function test_surface_is_translucent() {
        verify(Tokens.surface.a > 0 && Tokens.surface.a < 1);
    }

    function test_panel_motion_is_250ms() {
        compare(Tokens.durationPanel, 250);
    }
}
