import QtQuick
import QtTest
import "../media"
import "../theme"
import "../daemon/protocol.js" as Protocol
import "../media/media.js" as Media

// The now-playing pill: media.js on events in the shape novad sends, and
// the real pill and panel driven with them. The root is an Item: a
// TestCase root gets no mouse events.
Item {
    id: stage
    width: 600
    height: 600

    function player(id, status, extra) {
        return Object.assign({
            id: id,
            app: {
                id: "spotify",
                name: "Spotify"
            },
            identity: "Spotify",
            status: status,
            title: "Aruarian Dance",
            artist: "Nujabes",
            album: "Samurai Champloo: Departure",
            length_us: 238000000,
            position_us: 84000000,
            position_at_ms: 1000000,
            rate: 1,
            can_next: true,
            can_previous: true,
            can_play: true,
            can_pause: true,
            can_seek: true,
            can_raise: true
        }, extra ?? {});
    }

    function media(players, shown) {
        return {
            players: players,
            shown: shown
        };
    }

    TestCase {
        name: "MediaLogic"

        function test_shown_finds_the_player_novad_picked() {
            const spotify = stage.player("org.mpris.MediaPlayer2.spotify", Protocol.PLAYBACK_PLAYING);
            const mpv = stage.player("org.mpris.MediaPlayer2.mpv", Protocol.PLAYBACK_PAUSED);
            compare(Media.shown(stage.media([spotify, mpv], mpv.id)), mpv);
            compare(Media.shown(stage.media([spotify, mpv], undefined)), null);
            compare(Media.shown(stage.media([spotify], "gone")), null);
            compare(Media.shown(null), null);
        }

        function test_position_moves_on_only_while_playing() {
            const playing = stage.player("a", Protocol.PLAYBACK_PLAYING);
            // Two seconds after novad read it.
            compare(Media.position(playing, 1002000), 86000000);
            const paused = stage.player("a", Protocol.PLAYBACK_PAUSED);
            compare(Media.position(paused, 1002000), 84000000);
            const fast = stage.player("a", Protocol.PLAYBACK_PLAYING, {
                rate: 2
            });
            compare(Media.position(fast, 1002000), 88000000);
        }

        function test_position_stays_within_the_track() {
            const playing = stage.player("a", Protocol.PLAYBACK_PLAYING);
            compare(Media.position(playing, 1000000 + 3600 * 1000), 238000000);
            compare(Media.progress(playing, 1000000 + 3600 * 1000), 1);
            // A clock behind novad's doesn't move it back.
            compare(Media.position(playing, 0), 84000000);
            const unknown = stage.player("a", Protocol.PLAYBACK_PLAYING, {
                length_us: 0
            });
            compare(Media.progress(unknown, 1002000), 0);
            compare(Media.position(unknown, 1002000), 86000000);
        }

        function test_times_read_like_a_clock() {
            compare(Media.time(0), "0:00");
            compare(Media.time(84000000), "1:24");
            compare(Media.time(238000000), "3:58");
            compare(Media.time(3723000000), "1:02:03");
        }

        function test_seeking_stays_within_the_track() {
            const playing = stage.player("a", Protocol.PLAYBACK_PLAYING);
            compare(Media.seekTarget(playing, 0.5), 119000000);
            compare(Media.seekTarget(playing, -1), 0);
            compare(Media.seekTarget(playing, 2), 238000000);
        }

        function test_browsers_show_no_artist_in_the_pill() {
            verify(Media.pillArtist(stage.player("a", Protocol.PLAYBACK_PLAYING)));
            verify(!Media.pillArtist(stage.player("a", Protocol.PLAYBACK_PLAYING, {
                browser: true
            })));
            verify(!Media.pillArtist(stage.player("a", Protocol.PLAYBACK_PLAYING, {
                artist: ""
            })));
            verify(!Media.pillArtist(null));
        }

        function test_the_app_name_falls_back_to_the_players_own() {
            compare(Media.appName(stage.player("a", Protocol.PLAYBACK_PLAYING)), "Spotify");
            compare(Media.appName(stage.player("a", Protocol.PLAYBACK_PLAYING, {
                app: null,
                identity: "mpv Media Player"
            })), "mpv Media Player");
        }

        function test_chips_list_players_once_there_are_two_not_stopped() {
            const a = stage.player("a", Protocol.PLAYBACK_PLAYING);
            const b = stage.player("b", Protocol.PLAYBACK_PAUSED);
            const c = stage.player("c", Protocol.PLAYBACK_STOPPED);
            compare(Media.chips(stage.media([a], "a")).length, 0);
            compare(Media.chips(stage.media([a, c], "a")).length, 0);
            compare(Media.chips(stage.media([a, b, c], "a")).map(player => player.id), ["a", "b"]);
            compare(Media.chips(null).length, 0);
        }

        function test_equalizer_bars_stay_between_low_and_high() {
            compare(Media.equalizer(4, 3, 12, () => 0), [3, 3, 3, 3]);
            compare(Media.equalizer(4, 3, 12, () => 1), [12, 12, 12, 12]);
            compare(Media.equalizer(2, 3, 12, () => 0.5), [8, 8]);
        }

        function test_marquee_runs_at_its_speed() {
            compare(Media.marqueeDuration(60, 30), 2000);
            compare(Media.marqueeDuration(-5, 30), 0);
        }
    }

    TestCase {
        id: test
        name: "NowPlayingPill"
        when: windowShown

        Component {
            id: pillComponent
            NowPlaying {}
        }

        SignalSpy {
            id: controlSpy
            signalName: "controlRequested"
        }

        SignalSpy {
            id: showSpy
            signalName: "showRequested"
        }

        SignalSpy {
            id: focusSpy
            signalName: "focusRequested"
        }

        SignalSpy {
            id: workspaceSpy
            signalName: "workspaceAsked"
        }

        function init() {
            Tokens.apply({
                bg: "#1D1D1D",
                fg: "#F5E7C6",
                primary: "#FD7014"
            });
        }

        function createPill(media) {
            const pill = createTemporaryObject(pillComponent, stage, {
                media: media
            });
            for (const spy of [controlSpy, showSpy, focusSpy, workspaceSpy]) {
                spy.target = pill;
                spy.clear();
            }
            return pill;
        }

        function playing(id) {
            return stage.player(id ?? "org.mpris.MediaPlayer2.spotify", Protocol.PLAYBACK_PLAYING);
        }

        function test_nothing_playing_shows_nothing() {
            const pill = createPill(stage.media([], undefined));
            verify(!pill.visible);
            compare(pill.width, 0);
        }

        function test_nothing_playing_opens_no_panel() {
            const pill = createPill(stage.media([], undefined));
            pill.open = true;
            verify(!pill.open);
            verify(!pill.visible);
        }

        function test_a_playing_player_slides_the_pill_out() {
            const pill = createPill(stage.media([], undefined));
            const spotify = playing();
            pill.media = stage.media([spotify], spotify.id);
            tryVerify(() => pill.visible);
            tryCompare(pill, "shownT", 1);
            compare(pill.height, Tokens.barHeight);
            verify(pill.width > 100 && pill.width < 400, `pill width ${pill.width}`);
            // Gone: it slides away again.
            pill.media = stage.media([], undefined);
            tryCompare(pill, "visible", false);
        }

        function test_a_browser_pill_is_narrower_without_its_artist() {
            const song = createPill(stage.media([playing("a")], "a"));
            const video = createPill(stage.media([stage.player("a", Protocol.PLAYBACK_PLAYING, {
                    browser: true
                })], "a"));
            tryCompare(song, "shownT", 1);
            tryCompare(video, "shownT", 1);
            verify(video.width < song.width, `${video.width} < ${song.width}`);
        }

        function test_a_long_title_is_cut_to_its_room() {
            const pill = createPill(stage.media([stage.player("a", Protocol.PLAYBACK_PLAYING, {
                    title: "Everything In Its Right Place (Live in Paris, 2001, Remastered)"
                })], "a"));
            tryCompare(pill, "shownT", 1);
            verify(pill.faceWidth < Tokens.mediaTitleMax + Tokens.mediaArtistMax + 100, `face ${pill.faceWidth}`);
        }

        Component {
            id: titleComponent
            MarqueeTitle {
                text: "Everything In Its Right Place (Live in Paris, 2001, Remastered)"
            }
        }

        function test_a_long_title_scrolls_only_while_shown() {
            const title = createTemporaryObject(titleComponent, stage, {
                visible: false
            });
            verify(title.over > 0);
            // Its bar hidden beside a fullscreen window: nothing moves.
            title.scroll();
            verify(!title.moving);
            title.visible = true;
            title.scroll();
            verify(title.moving);
            // Hidden mid-scroll, it stops and rests at its start.
            title.visible = false;
            verify(!title.moving);
        }

        // Its fade held a window reference that nothing gave back, so a
        // removed screen left the title pointing at a deleted window, and
        // the UI crashed tearing it down (IMP-353).
        function test_a_long_title_leaves_the_window_with_its_parent() {
            const title = createTemporaryObject(titleComponent, stage);
            verify(title.over > 0);
            waitForRendering(title);
            title.parent = null;
            const inWindow = [];
            const visit = item => {
                if (item.Window.window !== null)
                    inWindow.push(item);
                for (const child of item.children)
                    visit(child);
            };
            visit(title);
            compare(inWindow.length, 0, `still in the window: ${inWindow}`);
        }

        function test_paused_it_lingers_and_then_slides_away() {
            const pill = createPill(stage.media([playing("a")], "a"));
            tryCompare(pill, "shownT", 1);
            pill.media = stage.media([stage.player("a", Protocol.PLAYBACK_PAUSED)], "a");
            verify(!pill.playing);
            verify(!pill.lingered);
            compare(pill.shownT, 1);
            // A minute later (the linger's own timer, run now).
            pill.lingered = true;
            tryCompare(pill, "visible", false);
            // Playing again brings it back.
            pill.media = stage.media([playing("a")], "a");
            verify(!pill.lingered);
            tryCompare(pill, "shownT", 1);
        }

        function test_an_open_panel_keeps_a_paused_pill() {
            const pill = createPill(stage.media([stage.player("a", Protocol.PLAYBACK_PAUSED)], "a"));
            pill.open = true;
            pill.lingered = true;
            verify(pill.visible);
            compare(pill.shownT, 1);
        }

        function test_clicking_opens_the_panel_and_asks_where_the_window_is() {
            const pill = createPill(stage.media([playing("a")], "a"));
            tryCompare(pill, "shownT", 1);
            compare(pill.shownPanel, null);
            mouseClick(pill, 20, Tokens.barHeight / 2);
            verify(pill.open);
            // Loaded as it opens, and fading in once the panel has grown.
            verify(pill.shownPanel.opacity < 1, `content opacity ${pill.shownPanel.opacity}`);
            compare(workspaceSpy.count, 1);
            compare(workspaceSpy.signalArguments[0][0], "a");
            tryCompare(pill, "width", pill.panelWidth);
            tryCompare(pill, "height", pill.openHeight);
            verify(pill.openHeight > 250, `open height ${pill.openHeight}`);
            // The whole header row closes it, past where the pill was too.
            mouseClick(pill, pill.panelWidth - 10, Tokens.barHeight / 2);
            verify(!pill.open);
            // Unloaded once closed.
            tryCompare(pill, "shownPanel", null);
        }

        function test_the_player_going_closes_the_panel() {
            const pill = createPill(stage.media([playing("a")], "a"));
            pill.open = true;
            pill.media = stage.media([], undefined);
            verify(!pill.open);
            // Another one shown instead keeps it open, asking about its window.
            const other = createPill(stage.media([playing("a"), playing("b")], "a"));
            workspaceSpy.target = other;
            workspaceSpy.clear();
            other.open = true;
            other.media = stage.media([playing("b")], "b");
            verify(other.open);
            compare(workspaceSpy.count, 2);
            compare(workspaceSpy.signalArguments[1][0], "b");
        }

        function test_the_buttons_reach_the_player() {
            const pill = createPill(stage.media([playing("a")], "a"));
            pill.open = true;
            tryVerify(() => !pill.moving);
            const play = findButton(pill, "Pause");
            verify(play, "play button");
            mouseClick(play);
            compare(controlSpy.count, 1);
            compare(controlSpy.signalArguments[0][0], "a");
            compare(controlSpy.signalArguments[0][1], Protocol.MEDIA_PLAY_PAUSE);
            mouseClick(findButton(pill, "Next track"));
            compare(controlSpy.signalArguments[1][1], Protocol.MEDIA_NEXT);
        }

        function test_clicking_the_progress_line_seeks_there() {
            const pill = createPill(stage.media([playing("a")], "a"));
            pill.open = true;
            tryVerify(() => !pill.moving);
            const line = findButton(pill, "Position");
            verify(line, "progress line");
            const seekSpy = createTemporaryObject(spyComponent, test, {
                target: pill,
                signalName: "seekRequested"
            });
            mouseClick(line, line.width / 2, line.height / 2);
            compare(seekSpy.count, 1);
            compare(seekSpy.signalArguments[0][0], "a");
            compare(seekSpy.signalArguments[0][1], 119000000);
        }

        Component {
            id: spyComponent
            SignalSpy {}
        }

        function test_a_button_the_player_cant_use_does_nothing() {
            const pill = createPill(stage.media([stage.player("a", Protocol.PLAYBACK_PLAYING, {
                    can_next: false
                })], "a"));
            pill.open = true;
            tryVerify(() => !pill.moving);
            const next = findButton(pill, "Next track");
            verify(!next.enabled);
            mouseClick(next);
            compare(controlSpy.count, 0);
        }

        function test_go_to_focuses_the_player_and_closes() {
            const pill = createPill(stage.media([playing("a")], "a"));
            pill.open = true;
            pill.workspace = "3";
            tryVerify(() => !pill.moving);
            const goTo = findButton(pill, "Go to Spotify on workspace 3");
            verify(goTo, "go-to row");
            mouseClick(goTo);
            compare(focusSpy.count, 1);
            compare(focusSpy.signalArguments[0][0], "a");
            verify(!pill.open);
        }

        function test_chips_show_another_player() {
            const pill = createPill(stage.media([playing("a"), stage.player("b", Protocol.PLAYBACK_PAUSED, {
                    app: {
                        id: "firefox",
                        name: "Firefox"
                    }
                })], "a"));
            pill.open = true;
            tryVerify(() => !pill.moving);
            const chip = findButton(pill, "Show Firefox");
            verify(chip, "Firefox chip");
            mouseClick(chip);
            compare(showSpy.count, 1);
            compare(showSpy.signalArguments[0][0], "b");
        }

        function test_one_player_has_no_chips() {
            const pill = createPill(stage.media([playing("a")], "a"));
            pill.open = true;
            tryVerify(() => !pill.moving);
            compare(findButton(pill, "Show Spotify, playing"), null);
        }

        // The item that reads as button `name` to a screen reader.
        function findButton(item, name) {
            if (item.Accessible && item.Accessible.name === name && item.visible)
                return item;
            for (let i = 0; i < item.children.length; i++) {
                const found = findButton(item.children[i], name);
                if (found)
                    return found;
            }
            return null;
        }
    }
}
