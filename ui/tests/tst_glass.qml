pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../theme"
import "../power"

// Glass's tint with and without a copy of the screen in its window. The
// copy here is a recorded frame; stage/Backdrop.qml, which takes real ones,
// is Quickshell's, which the VM tests cover.
TestCase {
    id: test
    name: "Glass"
    when: windowShown
    width: 400
    height: 300

    Image {
        id: frame
        source: "data/frame-400x300.png"
        visible: false
    }

    Component {
        id: backdropComponent
        GlassBackdrop {
            window: test.Window.window
            texture: frame
            ready: true
            origin: Qt.point(100, 40)
            screenWidth: 800
            screenHeight: 600
            skipTop: 30
        }
    }

    Component {
        id: glassComponent
        Glass {
            x: 50
            y: 20
            width: 200
            height: 80
            radius: 16
        }
    }

    Component {
        id: filletComponent
        EdgeFillet {
            width: 20
            height: 20
        }
    }

    function backdrop() {
        const made = createTemporaryObject(backdropComponent, test);
        GlassBackdrops.add(made);
        return made;
    }

    function cleanup() {
        for (const made of GlassBackdrops.all)
            GlassBackdrops.remove(made);
    }

    function fillOf(glass) {
        return glass.children.find(child => child instanceof GlassTint);
    }

    function test_without_a_copy_glass_keeps_the_fixed_tint() {
        const glass = createTemporaryObject(glassComponent, test);
        verify(!fillOf(glass).active);
        verify(!fillOf(glass).visible);
        verify(Qt.colorEqual(glass.color, Tokens.glass));
    }

    function test_with_a_copy_the_tint_draws_and_finds_its_place_on_the_screen() {
        const glass = createTemporaryObject(glassComponent, test);
        const copy = backdrop();
        const fill = fillOf(glass);
        verify(fill.active);
        compare(glass.color.a, 0);
        compare(fill.screenCopy, frame);
        // The test's window sits at the copy's origin.
        const at = glass.mapToItem(null, 0, 0);
        tryCompare(fill.onScreen, "x", at.x + copy.origin.x);
        compare(fill.onScreen.y, at.y + copy.origin.y);
        fuzzyCompare(fill.region.width, 200 / 800, 1e-6);
        fuzzyCompare(fill.region.height, 80 / 600, 1e-6);
        compare(fill.ready, 1);
        // The bar's row, plus a texel, is never looked at.
        fuzzyCompare(fill.floorY, 30 / 600 + 1 / Tokens.glassBackdropTexels.height, 1e-6);
    }

    function test_the_tint_follows_glass_that_moves() {
        const glass = createTemporaryObject(glassComponent, test);
        const copy = backdrop();
        const fill = fillOf(glass);
        glass.x = 120;
        tryCompare(fill.onScreen, "x", glass.mapToItem(null, 0, 0).x + copy.origin.x);
        glass.width = 100;
        tryCompare(fill.onScreen, "width", 100);
    }

    function test_glass_goes_back_to_the_fixed_tint_once_the_copy_is_gone() {
        const glass = createTemporaryObject(glassComponent, test);
        const copy = backdrop();
        verify(fillOf(glass).active);
        GlassBackdrops.remove(copy);
        verify(!fillOf(glass).active);
        verify(Qt.colorEqual(glass.color, Tokens.glass));
    }

    function test_a_copy_not_taken_yet_uses_the_fixed_thickness() {
        const glass = createTemporaryObject(glassComponent, test);
        const copy = backdrop();
        copy.ready = false;
        compare(fillOf(glass).ready, 0);
        compare(fillOf(glass).fallback, Tokens.glassOpacity);
    }

    function test_a_fillet_tints_with_its_corner_cut_out() {
        const fillet = createTemporaryObject(filletComponent, test, {
            above: false
        });
        const fill = fillOf(fillet);
        verify(!fill.active);
        backdrop();
        verify(fill.active);
        compare(fill.shape, 1);
        compare(fill.cut, Qt.vector4d(0, 20, 20, 0));
    }
}
