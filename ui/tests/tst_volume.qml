import QtQuick
import QtTest
import "../theme"
import "../volume"
import "../volume/volume.js" as Volume

// The capsule's logic (volume.js) and its label, without PipeWire or a
// window: those need Quickshell, which the VM tests cover.
TestCase {
    name: "Volume"
    when: windowShown

    function test_steps_land_on_the_grid_and_stop_at_100() {
        compare(Volume.stepped(0.64, 1, 0.05), 0.7);
        compare(Volume.stepped(0.65, 1, 0.05), 0.7);
        fuzzyCompare(Volume.stepped(0.62, -1, 0.05), 0.55, 1e-9);
        compare(Volume.stepped(0.98, 1, 0.05), 1);
        compare(Volume.stepped(0.02, -1, 0.05), 0);
    }

    function test_above_100_steps_down_from_100_and_not_up() {
        compare(Volume.stepped(1.2, 1, 0.05), 1.2);
        compare(Volume.stepped(1, 1, 0.05), 1);
        fuzzyCompare(Volume.stepped(1.2, -1, 0.05), 0.95, 1e-9);
    }

    function test_a_point_on_the_capsule_sets_whole_percents() {
        compare(Volume.atFraction(0.634), 0.63);
        compare(Volume.atFraction(1.3), 1);
        compare(Volume.atFraction(-0.2), 0);
    }

    function test_level_is_full_above_100_and_empty_without_an_output() {
        compare(Volume.level(1.2, true), 1);
        compare(Volume.level(0.4, true), 0.4);
        compare(Volume.level(0.4, false), 0);
    }

    function test_headphones_and_headsets_are_told_apart_from_speakers() {
        compare(Volume.outputKind(false, null), "none");
        compare(Volume.outputKind(true, null), "speakers");
        compare(Volume.outputKind(true, {
            "device.form-factor": "headset"
        }), "headphones");
        compare(Volume.outputKind(true, {
            "device.icon-name": "audio-headphones-bluetooth"
        }), "headphones");
        compare(Volume.outputKind(true, {
            "device.form-factor": "internal",
            "device.icon-name": "audio-card-analog-pci"
        }), "speakers");
    }

    // Default sinks' properties as PipeWire reports them (pw-cli i <id>),
    // trimmed to the ones outputKind() reads.
    function test_bluetooth_and_hdmi_outputs_are_told_apart() {
        compare(Volume.outputKind(true, {
            "node.name": "bluez_output.F8_4E_17_A1_22_9C.1",
            "node.description": "JBL Flip 6",
            "device.api": "bluez5",
            "device.icon-name": "audio-card-bluetooth"
        }), "bluetooth");
        compare(Volume.outputKind(true, {
            "node.name": "alsa_output.pci-0000_01_00.1.hdmi-stereo",
            "node.description": "GA104 High Definition Audio Controller Digital Stereo (HDMI)",
            "device.api": "alsa",
            "device.profile.name": "hdmi-stereo"
        }), "display");
        compare(Volume.outputKind(true, {
            "node.name": "alsa_output.pci-0000_0c_00.1.pro-output-3",
            "node.description": "Navi 21 HDMI/DP Audio Controller DisplayPort 1 Output",
            "device.api": "alsa"
        }), "display");
    }

    function test_headphones_win_over_bluetooth_and_usb_is_not_guessed() {
        compare(Volume.outputKind(true, {
            "node.name": "bluez_output.AC_80_0A_3B_19_E2.1",
            "device.api": "bluez5",
            "device.form-factor": "headset",
            "device.icon-name": "audio-headset-bluetooth"
        }), "headphones");
        compare(Volume.outputKind(true, {
            "node.name": "alsa_output.usb-Focusrite_Scarlett_2i2_USB-00.analog-stereo",
            "node.description": "Scarlett 2i2 USB Analog Stereo",
            "device.api": "alsa",
            "device.bus": "usb"
        }), "speakers");
        compare(Volume.outputKind(true, {
            "node.name": "alsa_output.pci-0000_0e_00.4.analog-stereo",
            "node.description": "Starship/Matisse HD Audio Controller Analog Stereo",
            "device.api": "alsa"
        }), "speakers");
    }

    function test_icons_follow_output_mute_and_volume() {
        compare(Volume.icon("none", false, 0), "none");
        compare(Volume.icon("speakers", true, 0.6), "muted");
        compare(Volume.icon("headphones", false, 0.6), "headphones");
        compare(Volume.icon("bluetooth", false, 0.6), "bluetooth");
        compare(Volume.icon("display", false, 0), "display");
        compare(Volume.icon("bluetooth", true, 0.6), "muted");
        compare(Volume.icon("speakers", false, 0), "off");
        compare(Volume.icon("speakers", false, 0.3), "low");
        compare(Volume.icon("speakers", false, 0.5), "high");
    }

    function test_the_first_state_seen_is_not_a_change() {
        verify(!Volume.isChange(null, {
            id: 40,
            volume: 0.5,
            muted: false
        }));
    }

    function test_volume_mute_and_a_new_output_are_changes() {
        const before = {
            id: 40,
            volume: 0.5,
            muted: false
        };
        verify(!Volume.isChange(before, Object.assign({}, before)));
        verify(Volume.isChange(before, Object.assign({}, before, {
            volume: 0.55
        })));
        verify(Volume.isChange(before, Object.assign({}, before, {
            muted: true
        })));
        verify(Volume.isChange(before, Object.assign({}, before, {
            id: 41
        })));
    }

    function test_a_spring_settles_on_its_target() {
        let state = {
            x: 0,
            v: 0
        };
        for (let i = 0; i < 240; i++)
            state = Volume.spring(state.x, state.v, 1, Tokens.springCapsule, Tokens.springCapsuleDamping, 1 / 120);
        fuzzyCompare(state.x, 1, 0.001);
        fuzzyCompare(state.v, 0, 0.01);
    }

    readonly property var size: ({
            width: Tokens.capsuleWidth,
            height: Tokens.capsuleHeight,
            gap: Tokens.capsuleEdgeGap,
            stretch: Tokens.capsuleStretch,
            overshoot: Tokens.capsuleOvershoot
        })

    function test_the_capsule_rests_a_gap_from_the_edge_and_is_hidden_past_it() {
        const out = Volume.box(1, 0, 200, 400, size);
        compare(out.x + out.w, 200 - Tokens.capsuleEdgeGap);
        compare(out.h, Tokens.capsuleHeight);
        compare(out.y, (400 - Tokens.capsuleHeight) / 2);
        compare(out.radius, Tokens.capsuleWidth / 2);
        verify(Volume.box(0, 0, 200, 400, size).x >= 200);
    }

    function test_it_never_outgrows_its_window() {
        const tallest = Volume.tallest(size);
        for (const open of [0, 0.5, 1, 1.2, 2])
            for (const speed of [0, 3, 6, 50])
                verify(Volume.box(open, speed, 200, 400, size).h <= tallest + 1e-9, `open ${open} speed ${speed}`);
    }

    function test_the_surface_is_the_bottom_when_empty() {
        compare(Volume.surfaceY(0, 260, 4), 260);
        compare(Volume.surfaceY(1, 260, 4), 4);
        compare(Volume.surfaceY(0.5, 260, 4), 130);
    }

    Component {
        id: labelComponent
        CapsuleLabel {
            width: Tokens.capsuleWidth
            height: Tokens.capsuleHeight
            glyph: "high"
            text: "64"
        }
    }

    function test_the_label_stays_put_while_the_liquid_moves() {
        const label = createTemporaryObject(labelComponent, this, {
            surfaceY: label_height()
        });
        verify(label);
        const face = label.face;
        // The icon and number sit at the bottom, above the label pad.
        compare(face.y + face.height, label.height - Tokens.capsuleLabelPad);
        // The surface crossing them, or above them, moves nothing.
        for (const surface of [face.y + face.height / 2, 0]) {
            label.surfaceY = surface;
            compare(label.face, face);
        }
    }

    function label_height(): real {
        return Tokens.capsuleHeight;
    }
}
