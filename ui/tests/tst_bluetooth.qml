import QtQuick
import QtTest
import "../quick_settings"
import "../theme"
import "../quick_settings/bluetooth.js" as Bluetooth

// The Bluetooth tile and page, driven with bluetooth events in the shape
// novad sends (crates/nova-protocol's wire format snapshot). The root is an
// Item: a TestCase root gets no mouse events.
Item {
    id: stage
    width: 360
    height: 900

    TestCase {
        name: "Bluetooth"
        when: windowShown

        Component {
            id: pageComponent
            BluetoothPage {
                width: 328
                height: 868
            }
        }
        Component {
            id: mainComponent
            MainPage {
                width: 328
                height: 868
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
                onAccent: "#1D1D1D",
                error: "#EE5396"
            });
            spy.target = null;
            spy.signalName = "";
            spy.clear();
        }

        // Where shot() saves what a page shows, to look at it; set it by hand
        // while working on the page.
        property string shotDir: ""
        function shot(item, file) {
            const image = grabImage(item);
            verify(image.width > 0);
            if (shotDir !== "")
                image.save(`${shotDir}/${file}.png`);
        }

        function headphones(status) {
            return {
                address: "00:1B:66:AA:BB:CC",
                name: "WH-1000XM5",
                kind: "headphones",
                status: status,
                battery: 80
            };
        }
        function phone(status) {
            return {
                address: "9C:2A:70:11:22:33",
                name: "Pixel 8",
                kind: "phone",
                status: status
            };
        }
        function speaker(status, error) {
            const device = {
                address: "C4:30:18:44:55:66",
                name: "JBL Flip 6",
                kind: "speaker",
                status: status
            };
            if (error)
                device.error = error;
            return device;
        }
        function event(devices, pairing) {
            return {
                radio: "on",
                scanning: true,
                devices: devices,
                pairing: pairing ?? null
            };
        }

        // The shown item named `name` under `item`: hidden prompts and cards
        // share names with the one in sight.
        function shown(item, name) {
            for (const child of item.children) {
                if (!child.visible)
                    continue;
                if (child.objectName === name)
                    return child;
                const found = shown(child, name);
                if (found)
                    return found;
            }
            return null;
        }
        function click(page, name) {
            // Positioners lay out what just showed on the next frame.
            waitForRendering(page);
            const item = shown(page, name);
            verify(item, `${name} shows`);
            mouseClick(item);
        }
        function listen(page, signalName) {
            spy.clear();
            spy.target = page;
            spy.signalName = signalName;
        }

        function test_the_tile_says_why_it_is_off_or_what_is_connected() {
            compare(Bluetooth.tileSubtitle(null), "No adapter");
            compare(Bluetooth.tileSubtitle({
                radio: "switched_off",
                devices: []
            }), "Off by hardware switch");
            compare(Bluetooth.tileSubtitle({
                radio: "off",
                devices: [phone("paired")]
            }), "Off");
            compare(Bluetooth.tileSubtitle(event([phone("paired")])), "On");
            compare(Bluetooth.tileSubtitle(event([headphones("connected"), phone("paired")])), "WH-1000XM5");
            compare(Bluetooth.tileSubtitle(event([headphones("connected"), phone("connected")])), "2 connected");
        }

        function test_devices_sort_into_connected_mine_and_nearby() {
            const devices = [headphones("connected"), phone("connecting"), speaker("pairing"), speaker("paired")];
            compare(Bluetooth.inSection(devices, "connected").length, 2, "on its way counts");
            compare(Bluetooth.inSection(devices, "mine").length, 1);
            compare(Bluetooth.inSection(devices, "nearby").length, 1, "one being paired stays nearby");
        }

        function test_codes_are_checked_like_novad_does() {
            const pin = {
                kind: "enter_pin"
            };
            const passkey = {
                kind: "enter_passkey"
            };
            verify(Bluetooth.validCode(pin, "0000"));
            verify(!Bluetooth.validCode(pin, ""));
            verify(!Bluetooth.validCode(pin, "has space"));
            verify(!Bluetooth.validCode(pin, "9".repeat(17)));
            verify(Bluetooth.validCode(passkey, "012345"));
            verify(!Bluetooth.validCode(passkey, "1234567"));
            verify(!Bluetooth.validCode(passkey, "12a4"));
        }

        function test_the_tile_toggles_and_its_arrow_opens_the_page() {
            const page = createTemporaryObject(mainComponent, stage, {
                bluetoothShown: true,
                bluetooth: event([headphones("connected")])
            });
            waitForRendering(page);
            const tile = findChild(page, "bluetoothTile");
            verify(tile.visible);
            listen(page, "bluetoothPowered");
            mouseClick(tile, 40, tile.height / 2);
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], false, "on turns off");

            listen(page, "pageRequested");
            mouseClick(tile, tile.width - 12, tile.height / 2);
            compare(spy.signalArguments[0][0], "bluetooth");

            // A hardware switch can't be turned on from here: the page says so.
            page.bluetooth = {
                radio: "switched_off",
                scanning: false,
                devices: [],
                pairing: null
            };
            spy.clear();
            mouseClick(tile, 40, tile.height / 2);
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], "bluetooth");

            page.bluetoothShown = false;
            verify(!tile.visible, "none with the feature off");
        }

        function test_one_click_connects_and_a_connected_one_offers_disconnect() {
            const page = createTemporaryObject(pageComponent, stage, {
                state: event([headphones("connected"), phone("paired"), speaker("nearby")])
            });
            waitForRendering(page);
            shot(page, "bluetooth");

            listen(page, "connectRequested");
            click(page, "device-C4:30:18:44:55:66");
            compare(spy.signalArguments[0][0], "C4:30:18:44:55:66", "a nearby one pairs and connects");
            click(page, "device-9C:2A:70:11:22:33");
            compare(spy.signalArguments[1][0], "9C:2A:70:11:22:33", "a known one connects");

            click(page, "device-00:1B:66:AA:BB:CC");
            compare(spy.count, 2, "a connected one doesn't connect again");
            listen(page, "disconnectRequested");
            click(page, "disconnect");
            compare(spy.signalArguments[0][0], "00:1B:66:AA:BB:CC");
            verify(!shown(page, "disconnect"), "it closes");
        }

        function test_forget_asks_first() {
            const page = createTemporaryObject(pageComponent, stage, {
                state: event([phone("paired")])
            });
            waitForRendering(page);
            listen(page, "forgetRequested");
            click(page, "more");
            verify(shown(page, "confirmForget"));
            click(page, "forgetConfirm");
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], "9C:2A:70:11:22:33");
        }

        function test_a_code_to_compare_shows_under_its_device() {
            const page = createTemporaryObject(pageComponent, stage, {
                state: event([headphones("connected"), phone("pairing")], {
                    id: 3,
                    address: "9C:2A:70:11:22:33",
                    ask: {
                        kind: "confirm",
                        code: "482913"
                    },
                    incoming: false
                })
            });
            waitForRendering(page);
            shot(page, "bluetooth-confirm");
            const prompt = shown(page, "code").parent.parent;
            const row = shown(page, "device-9C:2A:70:11:22:33");
            verify(prompt.mapToItem(page, 0, 0).y > row.mapToItem(page, 0, 0).y, "under the row");
            listen(page, "answered");
            click(page, "accept");
            compare(spy.signalArguments[0][0], 3);
            compare(spy.signalArguments[0][1], "accept");
            click(page, "decline");
            compare(spy.signalArguments[1][1], "reject");
        }

        function test_a_pin_is_typed_and_kept_while_rows_move() {
            const pairing = {
                id: 4,
                address: "C4:30:18:44:55:66",
                ask: {
                    kind: "enter_pin"
                },
                incoming: false
            };
            const page = createTemporaryObject(pageComponent, stage, {
                state: event([speaker("pairing")], pairing)
            });
            waitForRendering(page);
            const accept = shown(page, "accept");
            verify(!accept.enabled, "nothing typed yet");
            const field = shown(page, "field");
            field.forceActiveFocus();
            keyClick("1");
            keyClick("2");
            keyClick("3");
            keyClick("4");
            verify(accept.enabled);

            // A device found nearby sorts above it: the row it's in changes.
            page.state = event([
                {
                    address: "00:00:00:00:00:01",
                    name: "Bose",
                    kind: "speaker",
                    status: "nearby"
                },
                speaker("pairing")], pairing);
            waitForRendering(page);
            compare(shown(page, "field").text, "1234");

            listen(page, "answered");
            click(page, "accept");
            compare(spy.signalArguments[0][0], 4);
            compare(spy.signalArguments[0][1], {
                code: "1234"
            });
        }

        function test_a_failure_says_what_to_do_and_tries_again() {
            const words = "Couldn't reach it. Make sure it's on and nearby.";
            const page = createTemporaryObject(pageComponent, stage, {
                state: event([speaker("nearby", words)])
            });
            waitForRendering(page);
            shot(page, "bluetooth-failure");
            const failure = shown(page, "failure");
            verify(failure);
            compare(failure.text, words);
            listen(page, "connectRequested");
            click(page, "retry");
            compare(spy.count, 1, "Try again connects");
        }

        function test_paired_and_connected_says_so() {
            const page = createTemporaryObject(pageComponent, stage, {
                state: event([speaker("pairing")])
            });
            page.state = event([speaker("connecting")]);
            verify(!shown(page, "note"));
            page.state = event([speaker("connected")]);
            waitForRendering(page);
            const note = shown(page, "note");
            verify(note, "shows once connected");
        }

        function test_off_lists_known_devices_dimmed() {
            const page = createTemporaryObject(pageComponent, stage, {
                state: {
                    radio: "off",
                    scanning: false,
                    devices: [phone("paired")],
                    pairing: null
                }
            });
            waitForRendering(page);
            shot(page, "bluetooth-off");
            const row = shown(page, "device-9C:2A:70:11:22:33");
            verify(row);
            compare(row.opacity, Tokens.dimOpacity);
            listen(page, "connectRequested");
            mouseClick(row);
            compare(spy.count, 0, "nothing to connect while off");
            listen(page, "powerToggled");
            click(page, "power");
            compare(spy.signalArguments[0][0], true);
        }

        function test_a_hardware_switch_cannot_be_overridden() {
            const page = createTemporaryObject(pageComponent, stage, {
                state: {
                    radio: "switched_off",
                    scanning: false,
                    devices: [],
                    pairing: null
                }
            });
            waitForRendering(page);
            listen(page, "powerToggled");
            click(page, "power");
            compare(spy.count, 0);
        }
    }
}
