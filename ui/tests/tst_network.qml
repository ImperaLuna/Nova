import QtQuick
import QtTest
import "../quick_settings"
import "../theme"
import "../quick_settings/network.js" as Network

// The network tile and page, driven with novad's network event for the
// recording of RyzenShine (crates/novad/src/network/recorded, as
// ryzenshine_reads_as_its_cable_and_wifi reads it) and the states a
// connect goes through. The root is an Item: a TestCase root gets no mouse
// events.
Item {
    id: stage
    width: 360
    height: 900

    TestCase {
        name: "Network"
        when: windowShown

        Component {
            id: pageComponent
            NetworkPage {
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
                onAccent: "#1D1D1D"
            });
            spy.target = null;
            spy.signalName = "";
            spy.clear();
        }

        // RyzenShine as recorded: the cable and Wi-Fi both connected,
        // traffic over Wi-Fi.
        function recorded() {
            return {
                running: true,
                primary: "wifi",
                prefer: "wifi",
                wired: [
                    {
                        interface: "enp5s0",
                        state: "connected",
                        connection: "Wired connection 1",
                        speed_mbps: 1000
                    }
                ],
                wifi: {
                    interface: "wlp6s0",
                    enabled: true,
                    blocked: false,
                    state: "connected",
                    network: "Home",
                    speed_mbps: 585,
                    scanning: false,
                    networks: [
                        {
                            ssid: "Home",
                            strength: 69,
                            security: "wpa2",
                            band: "5ghz",
                            saved: true,
                            connected: true
                        }
                    ]
                }
            };
        }

        // After a rescan (nm-rescan.jsonl): a neighbor's network too, plus
        // a work network Nova leaves to the system's settings.
        function rescanned() {
            const network = recorded();
            network.wifi.networks.push({
                ssid: "Neighbor-1",
                strength: 40,
                security: "wpa2",
                band: "2.4ghz",
                saved: false,
                connected: false
            }, {
                ssid: "eduroam",
                strength: 20,
                security: "enterprise",
                saved: false,
                connected: false
            });
            return network;
        }

        function withAttempt(network, ssid, outcome) {
            network.wifi.attempt = {
                ssid: ssid,
                outcome: outcome
            };
            return network;
        }

        function test_both_links_up_name_the_one_in_use_and_the_backup() {
            const network = recorded();
            compare(Network.role(network, "wifi"), "use");
            compare(Network.role(network, "wired"), "backup");
            network.primary = "wired";
            compare(Network.role(network, "wired"), "use");
            compare(Network.role(network, "wifi"), "backup");
        }

        function test_wifi_alone_is_connected_without_a_cable() {
            const network = recorded();
            network.primary = "wifi";
            network.wired[0].state = "unavailable";
            compare(Network.wiredLink(network), null);
            compare(Network.role(network, "wifi"), "connected");
            verify(!Network.preferShown(network));
        }

        function test_network_lines() {
            const network = rescanned();
            compare(Network.networkLine(network, network.wifi.networks[0]), "5 GHz");
            compare(Network.networkLine(network, network.wifi.networks[1]), "2.4 GHz");
            compare(Network.networkLine(network, network.wifi.networks[2]), "Enterprise");
            network.wifi.networks[1].saved = true;
            compare(Network.networkLine(network, network.wifi.networks[1]), "Saved · 2.4 GHz");
            withAttempt(network, "Neighbor-1", "failed");
            compare(Network.networkLine(network, network.wifi.networks[1]), "Couldn't connect");
        }

        function test_passwords_count_bytes_like_novad() {
            compare(Network.passwordBytes("abc"), 3);
            compare(Network.passwordBytes("é"), 2);
            compare(Network.passwordBytes("€"), 3);
            compare(Network.passwordBytes("😀"), 4);
            verify(!Network.passwordFits("1234567"));
            verify(Network.passwordFits("12345678"));
            verify(Network.passwordFits("x".repeat(128)));
            verify(!Network.passwordFits("x".repeat(128) + "é"));
        }

        function test_speeds_read_short() {
            compare(Network.linkSpeed(1000), "1 Gb/s");
            compare(Network.linkSpeed(2500), "2.5 Gb/s");
            compare(Network.linkSpeed(585), "585 Mb/s");
            compare(Network.speed(3.5 * 1024 * 1024), "3.5 MB/s");
            compare(Network.speed(114 * 1024), "114 KB/s");
            compare(Network.arcs(69), 3);
            compare(Network.arcs(40), 2);
            compare(Network.arcs(20), 1);
        }

        function test_the_tile_lights_while_online_and_opens_the_page() {
            const page = createTemporaryObject(mainComponent, stage, {
                networkShown: true,
                network: recorded()
            });
            const tile = findChild(page, "networkTile");
            verify(tile.visible);
            verify(tile.on);
            spy.target = page;
            spy.signalName = "pageRequested";
            mouseClick(tile, tile.width / 3, tile.height / 2);
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], "network");
        }

        function test_the_tile_waits_for_its_feature() {
            const page = createTemporaryObject(mainComponent, stage);
            verify(!findChild(page, "networkTile").visible);
        }

        function test_page_shows_both_links_as_recorded() {
            const page = createTemporaryObject(pageComponent, stage, {
                network: recorded(),
                traffic: {
                    down: [1000, 3670016],
                    up: [10, 116736],
                    today_down: 4509715660,
                    today_up: 325058560
                }
            });
            compare(findChild(page, "linkName").text, "Home");
            compare(findChild(page, "down").children[1].text, "↓ 3.5 MB/s");
            compare(findChild(page, "today").text, "today ↓ 4.2 GB ↑ 310 MB");
            verify(findChild(page, "prefer").visible);
            compare(findChild(page, "prefer").current, "wifi");
            const wired = findChild(page, "wired");
            verify(wired.visible);
            compare(wired.role, "backup");
            compare(findChild(page, "network-Home").role, "use");
        }

        function test_a_long_network_name_ends_in_an_ellipsis_before_the_link_speed() {
            // SSIDs run up to 32 bytes.
            const ssid = "Vodafone-Homespot-5G-Extended-WW";
            const network = recorded();
            network.wifi.network = ssid;
            network.wifi.networks[0].ssid = ssid;
            const page = createTemporaryObject(pageComponent, stage, {
                network: network
            });
            const name = findChild(page, "linkName");
            compare(name.text, ssid);
            tryVerify(() => name.truncated, 1000, "the name is cut short");
            const link = name.parent;
            verify(link.width <= link.parent.width, "the speed stays inside the card");
        }

        function test_prefer_sends_the_pick() {
            const page = createTemporaryObject(pageComponent, stage, {
                network: recorded()
            });
            spy.target = page;
            spy.signalName = "preferPicked";
            const prefer = findChild(page, "prefer");
            mouseClick(prefer, prefer.width / 4, prefer.height / 2);
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], "wired");
        }

        function test_without_a_cable_the_wired_row_and_prefer_hide() {
            const network = recorded();
            network.wired[0] = {
                interface: "enp5s0",
                state: "unavailable"
            };
            const page = createTemporaryObject(pageComponent, stage, {
                network: network
            });
            verify(!findChild(page, "wired").visible);
            verify(!findChild(page, "prefer").visible);
            compare(findChild(page, "network-Home").role, "connected");
        }

        function test_a_new_network_asks_for_its_password_then_joins() {
            const page = createTemporaryObject(pageComponent, stage, {
                network: rescanned()
            });
            spy.target = page;
            spy.signalName = "connectRequested";
            const row = findChild(page, "network-Neighbor-1");
            mouseClick(row);
            compare(spy.count, 0, "a secured network that isn't saved asks first");
            const field = findChild(row.parent, "password");
            verify(field.visible);
            tryVerify(() => field.activeFocus);
            verify(!findChild(row.parent, "connect").enabled);
            keyClick("s");
            keyClick("h");
            keyClick(Qt.Key_Return);
            compare(spy.count, 0, "too short for WPA");
            for (const key of "orthorse")
                keyClick(key);
            verify(findChild(row.parent, "connect").enabled);
            keyClick(Qt.Key_Return);
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], "Neighbor-1");
            compare(spy.signalArguments[0][1], "shorthorse");
            verify(!field.visible);
        }

        function test_a_wrong_password_opens_the_field_again_with_the_text_kept() {
            const page = createTemporaryObject(pageComponent, stage, {
                network: rescanned()
            });
            const row = findChild(page, "network-Neighbor-1");
            mouseClick(row);
            for (const key of "wrongpass")
                keyClick(key);
            keyClick(Qt.Key_Return);
            page.network = withAttempt(rescanned(), "Neighbor-1", "connecting");
            compare(page.connecting, "Neighbor-1");
            page.network = withAttempt(rescanned(), "Neighbor-1", "wrong_password");
            const field = findChild(row.parent, "password");
            verify(field.visible);
            compare(field.text, "wrongpass");
            verify(findChild(row.parent, "wrong").visible);
            tryVerify(() => field.activeFocus);
            keyClick(Qt.Key_Backspace);
            verify(!findChild(row.parent, "wrong").visible, "typing clears it");
            keyClick(Qt.Key_Escape);
            verify(!field.visible);
            page.network = withAttempt(rescanned(), "Neighbor-1", "wrong_password");
            verify(!field.visible, "the same attempt again doesn't reopen it");
        }

        function test_a_joined_network_doesnt_clear_a_password_typed_later() {
            const page = createTemporaryObject(pageComponent, stage, {
                network: rescanned()
            });
            page.network = withAttempt(rescanned(), "Home", "connected");
            const row = findChild(page, "network-Neighbor-1");
            mouseClick(row);
            for (const key of "longenough")
                keyClick(key);
            const network = withAttempt(rescanned(), "Home", "connected");
            network.wifi.networks[0].strength = 80;
            page.network = network;
            compare(findChild(row.parent, "password").text, "longenough");
        }

        function test_a_wrong_password_from_before_the_page_opened_stays_shut() {
            const page = createTemporaryObject(pageComponent, stage, {
                network: withAttempt(rescanned(), "Neighbor-1", "wrong_password")
            });
            verify(!findChild(findChild(page, "network-Neighbor-1").parent, "password").visible);
        }

        function test_a_saved_network_joins_at_once() {
            const network = rescanned();
            network.wifi.networks[1].saved = true;
            const page = createTemporaryObject(pageComponent, stage, {
                network: network
            });
            spy.target = page;
            spy.signalName = "connectRequested";
            mouseClick(findChild(page, "network-Neighbor-1"));
            compare(spy.count, 1);
            compare(spy.signalArguments[0][1], null);
        }

        function test_enterprise_networks_are_listed_but_not_joined() {
            const page = createTemporaryObject(pageComponent, stage, {
                network: rescanned()
            });
            spy.target = page;
            spy.signalName = "connectRequested";
            const row = findChild(page, "network-eduroam");
            verify(row.visible);
            mouseClick(row);
            compare(spy.count, 0);
            verify(!findChild(row.parent, "password").visible);
        }

        function test_the_network_it_is_on_disconnects_and_forgets_from_its_menu() {
            const page = createTemporaryObject(pageComponent, stage, {
                network: recorded()
            });
            const row = findChild(page, "network-Home");
            const more = findChild(row, "more");
            verify(more.visible, "the one it's on shows ⋯ without a hover");
            mouseClick(more);
            spy.target = page;
            spy.signalName = "forgetRequested";
            const forget = findChild(page, "forget");
            verify(forget.visible);
            waitForItemPolished(forget.parent);
            verify(findChild(page, "disconnect").visible);
            mouseClick(forget);
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], "Home");
            verify(!findChild(page, "menu").visible);
        }

        function test_a_work_network_its_on_still_disconnects() {
            const network = rescanned();
            network.wifi.network = "eduroam";
            network.wifi.networks = [Object.assign(network.wifi.networks[2], {
                    saved: true,
                    connected: true
                })];
            const page = createTemporaryObject(pageComponent, stage, {
                network: network
            });
            const more = findChild(findChild(page, "network-eduroam"), "more");
            verify(more.visible);
            mouseClick(more);
            spy.target = page;
            spy.signalName = "disconnectRequested";
            const disconnect = findChild(page, "disconnect");
            waitForItemPolished(disconnect.parent);
            mouseClick(disconnect);
            compare(spy.count, 1);
        }

        function test_a_blocked_switch_doesnt_turn_wifi_on() {
            const network = recorded();
            network.primary = "wired";
            network.wifi.enabled = false;
            network.wifi.blocked = true;
            network.wifi.state = "unavailable";
            network.wifi.networks = [];
            delete network.wifi.network;
            const page = createTemporaryObject(pageComponent, stage, {
                network: network
            });
            spy.target = page;
            spy.signalName = "wifiToggled";
            mouseClick(findChild(page, "wifiSwitch"));
            compare(spy.count, 0);
            verify(!findChild(page, "networks").visible);
            network.wifi.blocked = false;
            page.network = JSON.parse(JSON.stringify(network));
            mouseClick(findChild(page, "wifiSwitch"));
            compare(spy.count, 1);
            compare(spy.signalArguments[0][0], true);
        }

        function test_no_wifi_card_leaves_the_cable() {
            const network = recorded();
            network.primary = "wired";
            delete network.prefer;
            delete network.wifi;
            const page = createTemporaryObject(pageComponent, stage, {
                network: network
            });
            compare(findChild(page, "linkName").text, "Wired");
            verify(findChild(page, "wired").visible);
            compare(findChild(page, "wired").role, "use");
            verify(!findChild(page, "prefer").visible);
            verify(!findChild(page, "wifiSwitch").visible);
            verify(!findChild(page, "networks").visible);
        }
    }
}
