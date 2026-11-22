//! The launcher: listing, searching, launching and focusing apps.

use super::*;

/// Writes an app's desktop entry where the test daemon finds apps.
fn install_app(novad: &TestDaemon, file: &str, entry: &str) {
    let path = novad.dir.join("launcher/share/applications").join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Written beside it and moved in, as package managers do.
    let partial = path.with_extension("partial");
    std::fs::write(&partial, format!("[Desktop Entry]\nType=Application\n{entry}")).unwrap();
    std::fs::rename(partial, path).unwrap();
}

/// Puts icon `name` where the test daemon finds icons, and returns its file.
fn install_icon(novad: &TestDaemon, name: &str) -> String {
    let path = novad.dir.join("launcher/share/pixmaps").join(format!("{name}.png"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "").unwrap();
    path.display().to_string()
}

impl Client {
    /// Subscribes to the launcher and returns its data once it lists `apps` apps.
    fn subscribe_launcher(&mut self, apps: usize) -> Value {
        self.request(1, "subscribe", Some(json!({"topics": ["launcher"]})));
        let first = self.next_event("launcher");
        match first["apps"].as_array().unwrap().len() == apps {
            true => first,
            false => self.launcher_until(|event| event["apps"].as_array().unwrap().len() == apps),
        }
    }

    /// The next launcher event that `done` accepts.
    fn launcher_until(&mut self, done: impl Fn(&Value) -> bool) -> Value {
        loop {
            let event = self.next_event("launcher");
            if done(&event) {
                return event;
            }
        }
    }
}

fn ids(list: &Value) -> Vec<&str> {
    list.as_array().unwrap().iter().map(|item| item.as_str().unwrap_or_else(|| item["id"].as_str().unwrap())).collect()
}

#[test]
fn the_launcher_lists_apps_and_follows_installs() {
    let novad = TestDaemon::with_launcher("launcher-apps");
    let foot_icon = install_icon(&novad, "foot");
    install_app(&novad, "foot.desktop", "Name=Foot\nGenericName=Terminal\nIcon=foot\nExec=foot\n");
    install_app(&novad, "hidden.desktop", "Name=Hidden\nNoDisplay=true\nExec=x\n");
    let mut client = novad.connect();
    let first = client.subscribe_launcher(1);
    assert_eq!(
        first["apps"],
        json!([{"id": "foot.desktop", "name": "Foot", "description": "Terminal", "icon": foot_icon}])
    );

    // A new app, in a folder made after novad started, shows up.
    install_app(&novad, "org.kde/dolphin.desktop", "Name=Dolphin\nExec=dolphin %u\n");
    let event = client.launcher_until(|event| event["apps"].as_array().unwrap().len() == 2);
    assert_eq!(ids(&event["apps"]), ["org.kde-dolphin.desktop", "foot.desktop"], "sorted by name");
    // And one that's removed goes.
    std::fs::remove_file(novad.dir.join("launcher/share/applications/foot.desktop")).unwrap();
    client.launcher_until(|event| ids(&event["apps"]) == ["org.kde-dolphin.desktop"]);

    // A whole new applications folder renamed in place of the old one is
    // read, and so are apps installed into it afterwards.
    let applications = novad.dir.join("launcher/share/applications");
    let replacement = novad.dir.join("launcher/share/applications.new");
    std::fs::create_dir_all(&replacement).unwrap();
    std::fs::write(replacement.join("a.desktop"), "[Desktop Entry]\nType=Application\nName=A\nExec=a\n").unwrap();
    std::fs::rename(&applications, novad.dir.join("launcher/share/applications.old")).unwrap();
    std::fs::rename(&replacement, &applications).unwrap();
    client.launcher_until(|event| ids(&event["apps"]) == ["a.desktop"]);
    install_app(&novad, "b.desktop", "Name=B\nExec=b\n");
    client.launcher_until(|event| ids(&event["apps"]) == ["a.desktop", "b.desktop"]);
    // Moved away with nothing in its place, its apps go.
    std::fs::rename(&applications, novad.dir.join("launcher/share/gone")).unwrap();
    client.launcher_until(|event| event["apps"].as_array().unwrap().is_empty());
}

#[test]
fn an_icon_installed_after_its_app_shows() {
    let novad = TestDaemon::with_launcher("launcher-late-icon");
    install_app(&novad, "game.desktop", "Name=Game\nExec=game\nIcon=steam_icon_42\n");
    let mut client = novad.connect();
    let first = client.subscribe_launcher(1);
    assert_eq!(first["apps"][0].get("icon"), None);
    // Like Steam, which writes a game's icon after its shortcut.
    let icon = install_icon(&novad, "steam_icon_42");
    client.launcher_until(|event| event["apps"][0]["icon"] == icon);
}

#[test]
fn the_launcher_follows_apps_behind_a_linked_subfolder() {
    let novad = TestDaemon::with_launcher("launcher-linked");
    let vendor = novad.dir.join("vendor-apps");
    std::fs::create_dir_all(&vendor).unwrap();
    install_app(&novad, "foot.desktop", "Name=Foot\nExec=foot\n");
    std::os::unix::fs::symlink(&vendor, novad.dir.join("launcher/share/applications/vendor")).unwrap();
    let mut client = novad.connect();
    client.subscribe_launcher(1);
    std::fs::write(vendor.join("a.desktop"), "[Desktop Entry]\nType=Application\nName=A\nExec=a\n").unwrap();
    client.launcher_until(|event| ids(&event["apps"]) == ["vendor-a.desktop", "foot.desktop"]);
}

#[test]
fn launcher_searches_rank_apps_and_refuse_long_queries() {
    let novad = TestDaemon::with_launcher("launcher-search");
    install_app(&novad, "firefox.desktop", "Name=Firefox\nGenericName=Web Browser\nExec=firefox\n");
    install_app(&novad, "files.desktop", "Name=Files\nKeywords=folder;\nExec=nautilus\n");
    install_app(&novad, "fooview.desktop", "Name=Fooview\nExec=fooview\n");
    let mut client = novad.connect();
    client.subscribe_launcher(3);

    let found = client.request(2, "launcher_search", Some(json!({"query": " FI "})));
    // Fooview has f and i in order, which counts least.
    assert_eq!(ids(&found["result"]), ["files.desktop", "firefox.desktop", "fooview.desktop"]);
    assert_eq!(found["result"][0]["name"], json!([{"text": "Fi", "matched": true}, {"text": "les"}]));
    let by_keyword = client.request(3, "launcher_search", Some(json!({"query": "folder"})));
    assert_eq!(by_keyword["result"], json!([{"provider": "apps", "id": "files.desktop", "name": [{"text": "Files"}]}]));
    assert_eq!(client.request(4, "launcher_search", Some(json!({"query": ""})))["result"], json!([]));
    assert_eq!(client.request(5, "launcher_search", Some(json!({"query": "zzz"})))["result"], json!([]));
    let max = nova_launcher_api::LAUNCHER_SEARCH_MAX_CHARS;
    let long = client.request(6, "launcher_search", Some(json!({"query": "x".repeat(max + 1)})));
    assert!(long["error"].as_str().unwrap().contains(&format!("at most {max}")));
}

#[test]
fn app_streams_get_their_apps_icons_by_program_and_name() {
    let novad = TestDaemon::with_launcher("app-icon");
    let [zen, claude, firefox, mpv, audio] =
        ["zen-browser", "claude", "firefox-icon", "mpv", "audio-x-generic"].map(|name| install_icon(&novad, name));
    install_app(&novad, "zen-beta.desktop", "Name=Zen Browser\nExec=zen-beta\nIcon=zen-browser\n");
    install_app(&novad, "com.anthropic.Claude.desktop", "Name=Claude\nExec=claude-desktop\nIcon=claude\n");
    install_app(&novad, "firefox.desktop", "Name=Firefox\nExec=firefox\nIcon=firefox-icon\n");
    let mut client = novad.connect();
    client.subscribe_launcher(3);
    let icon = |client: &mut Client, id, [icon_name, program, name]: [&str; 3]| {
        client.request(id, "app_icon", Some(json!({"icon_name": icon_name, "program": program, "name": name})))
    };

    // Found loosely: Zen plays as `.zen-wrapped`, its entry is "Zen Browser".
    assert_eq!(icon(&mut client, 2, ["", "/nix/store/x-zen/lib/zen/.zen-wrapped", "Zen"])["result"], zen);
    // By the icon name the stream gives; Electron says nothing.
    assert_eq!(
        icon(&mut client, 3, ["com.anthropic.Claude", "/usr/lib/electron/electron", "Claude"])["result"],
        claude
    );
    assert_eq!(icon(&mut client, 4, ["", "/usr/bin/firefox", "Firefox"])["result"], firefox);
    // A browser's web app (`zen-<id>.desktop`) isn't the browser.
    install_app(&novad, "zen-abc123-Default.desktop", "Name=YouTube\nExec=zen-beta --app=abc123\nIcon=youtube\n");
    client.launcher_until(|event| event["apps"].as_array().unwrap().len() == 4);
    assert_eq!(icon(&mut client, 5, ["", "/nix/store/x-zen/lib/zen/.zen-wrapped", "Zen"])["result"], zen);
    // No app: the icon theme's for the icon name it gives, else its program.
    assert_eq!(icon(&mut client, 6, ["", "/usr/bin/mpv", "mpv"])["result"], mpv);
    assert_eq!(icon(&mut client, 7, ["audio-x-generic", "/usr/bin/mpv", "mpv"])["result"], audio);
    assert_eq!(icon(&mut client, 8, ["not-installed", "/usr/bin/mpv", "mpv"])["result"], mpv);
    assert_eq!(icon(&mut client, 9, ["", "", "Some Game"])["result"], json!(null));
    // A path isn't an icon name.
    assert_eq!(icon(&mut client, 10, [&mpv, "", ""])["result"], json!(null));
    let long = icon(&mut client, 11, ["", &"a".repeat(513), ""]);
    assert!(long["error"].as_str().unwrap().contains("at most 512"));
}

#[test]
fn launching_runs_the_app_without_a_shell_and_makes_it_recent() {
    let novad = TestDaemon::with_launcher("launcher-launch");
    let marker = novad.dir.join("launched");
    // %U is dropped; the semicolon would only end a command in a shell, and
    // lands in Path= as a file.
    let entry = format!("Name=Toucher\nExec=touch \"{}\" %U ;\nPath={}\n", marker.display(), novad.dir.display());
    install_app(&novad, "touch.desktop", &entry);
    // cp copies what the kernel says about it: its session.
    let stat = novad.dir.join("stat");
    install_app(&novad, "other.desktop", &format!("Name=Other\nExec=cp /proc/self/stat \"{}\"\n", stat.display()));
    let mut client = novad.connect();
    client.subscribe_launcher(2);

    assert_eq!(
        client.request(2, "launcher_run", Some(json!({"provider": "apps", "id": "touch.desktop"})))["result"],
        Value::Null
    );
    let event = client.launcher_until(|event| !event["recent"].as_array().unwrap().is_empty());
    assert_eq!(event["recent"], json!(["touch.desktop"]));
    let ran = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        marker.exists()
    });
    assert!(ran, "the app didn't run");
    assert!(novad.dir.join(";").exists(), "`;` reached touch as a file name, not a shell");

    // No Path= and a HOME that may not exist (Nix's builds): it starts in `/`.
    assert_eq!(
        client.request(3, "launcher_run", Some(json!({"provider": "apps", "id": "other.desktop"})))["result"],
        Value::Null
    );
    let event = client.launcher_until(|event| event["recent"].as_array().unwrap().len() == 2);
    assert_eq!(event["recent"], json!(["other.desktop", "touch.desktop"]), "the latest first");
    // A pinned app shows among the pins only, and is recent again unpinned.
    client.request(5, "pin_app", Some(json!({"id": "touch.desktop", "pinned": true})));
    let event = client.launcher_until(|event| event["recent"].as_array().unwrap().len() == 1);
    assert_eq!(event["recent"], json!(["other.desktop"]));
    client.request(6, "pin_app", Some(json!({"id": "touch.desktop", "pinned": false})));
    client.launcher_until(|event| event["recent"] == json!(["other.desktop", "touch.desktop"]));
    // Another session, away from novad's terminal (an app touching it would
    // be stopped as a background job): stat's 6th field, after the name.
    let copied = (0..100).find_map(|_| {
        std::thread::sleep(Duration::from_millis(50));
        std::fs::read_to_string(&stat).ok().filter(|text| text.ends_with('\n'))
    });
    let copied = copied.expect("the app didn't run");
    let fields: Vec<&str> = copied.rsplit_once(") ").unwrap().1.split(' ').collect();
    // SAFETY: getsid only reads.
    let ours = unsafe { libc::getsid(0) }.to_string();
    assert_ne!(fields[3], ours, "in novad's session: {copied}");
    let missing = client.request(7, "launcher_run", Some(json!({"provider": "apps", "id": "nothing.desktop"})));
    assert!(missing["error"].as_str().unwrap().contains("no app"));
}

#[test]
fn a_launched_app_outlives_the_request_that_started_it() {
    let novad = TestDaemon::with_launcher("launcher-outlives");
    // Like bubblewrap's --die-with-parent (Claude Desktop): killed when its
    // parent thread ends, here the one that answered the request.
    let pid_file = novad.dir.join("pid");
    let script = write_program(&novad.dir, "app.sh", &format!("echo $$ > \"{}\"\nexec sleep 30", pid_file.display()));
    install_app(
        &novad,
        "doomed.desktop",
        &format!("Name=Doomed\nExec=setpriv --pdeathsig KILL {}\n", script.display()),
    );
    let mut client = novad.connect();
    client.subscribe_launcher(1);
    assert_eq!(
        client.request(2, "launcher_run", Some(json!({"provider": "apps", "id": "doomed.desktop"})))["result"],
        Value::Null
    );
    let pid = (0..100).find_map(|_| {
        std::thread::sleep(Duration::from_millis(50));
        std::fs::read_to_string(&pid_file).ok().and_then(|text| text.trim().parse::<i32>().ok())
    });
    let pid = pid.expect("the app didn't run");
    drop(client);
    std::thread::sleep(Duration::from_millis(500));
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    let field = |name: &str| {
        status.lines().find_map(|line| line.strip_prefix(name)).map(str::trim).unwrap_or_default().to_owned()
    };
    let alive = !status.is_empty() && !field("State:").starts_with('Z');
    // SAFETY: kill only sends a signal.
    unsafe { libc::kill(pid, libc::SIGKILL) };
    assert!(alive, "the app died with the thread that started it");
    assert_ne!(field("PPid:"), std::process::id().to_string(), "the app is novad's child");
}

/// Launches the app the recording's window belongs to (`nova-second`, on a
/// workspace the focused output doesn't show: niri's `web`, Hyprland's
/// hidden special workspace) from the focused output, and returns the actions taken
/// and whether the app ran anyway, then the same asking for a new window.
fn focus_the_recorded_window(
    name: &str,
    fake: fn(&Path) -> (FakeCompositor, Box<dyn nova_compositor::Backend>),
) -> (Vec<String>, Value, bool) {
    let mut compositor = None;
    let novad = TestDaemon::with_launcher_following(name, |dir| {
        let (fake, backend) = fake(dir);
        compositor = Some(fake);
        backend
    });
    let compositor = compositor.unwrap();
    let marker = novad.dir.join("ran");
    let entry = format!("Name=Second\nExec=touch \"{}\"\nStartupWMClass=Nova-Second\n", marker.display());
    install_app(&novad, "second.desktop", &entry);
    let (mut client, desktop) = follow_to_the_end(&novad);
    client.subscribe_launcher(1);
    let output = focused_output(&desktop);
    let launch = json!({"provider": "apps", "id": "second.desktop", "output": output});
    assert_eq!(client.request(2, "launcher_run", Some(launch))["result"], Value::Null);
    let event = client.launcher_until(|event| !event["recent"].as_array().unwrap().is_empty());
    assert_eq!(event["recent"], json!(["second.desktop"]), "focusing it counts as a launch");
    let actions = compositor.actions();

    let fresh = json!({"provider": "apps", "id": "second.desktop", "output": output, "new_window": true});
    assert_eq!(client.request(3, "launcher_run", Some(fresh))["result"], Value::Null);
    let ran = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        marker.exists()
    });
    assert_eq!(compositor.actions(), actions, "a new window leaves the open one alone");
    let shown =
        desktop["outputs"].as_array().unwrap().iter().find(|o| o["name"] == output.as_str()).unwrap()["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|workspace| workspace["active"] == true)
            .unwrap()
            .clone();
    (actions, shown, ran)
}

#[test]
fn launching_an_open_app_on_niri_focuses_its_window_where_it_is() {
    let (actions, shown, ran) = focus_the_recorded_window("launcher-focus-niri", FakeCompositor::niri);
    let focused = json!({"Action": {"FocusWindow": {"id": 3}}});
    assert_ne!(shown["name"], "web", "the recording shows another workspace there");
    assert_eq!(actions, [focused.to_string()], "the window stays on its workspace");
    assert!(ran, "asked for a new window, the app runs");
}

#[test]
fn launching_an_open_app_on_hyprland_focuses_its_window_where_it_is() {
    let (actions, shown, ran) = focus_the_recorded_window("launcher-focus-hyprland", FakeCompositor::hyprland);
    assert_ne!(shown["name"], "web", "the recording shows another workspace there");
    let steps: Vec<Value> = serde_json::from_str(&FakeCompositor::recorded("hyprland", "steps.json")).unwrap();
    let clients = FakeCompositor::recorded("hyprland", &format!("{:02}/clients.json", steps.len() - 1));
    let clients: Value = serde_json::from_str(&clients).unwrap();
    let second = clients.as_array().unwrap().iter().find(|client| client["class"] == "nova-second").unwrap();
    let address = second["address"].as_str().unwrap();
    assert_eq!(actions, [format!("dispatch focuswindow address:{address}")], "the window stays on its workspace");
    assert!(ran, "asked for a new window, the app runs");

    let (actions, _, _) = focus_the_recorded_window("launcher-focus-hyprland-lua", FakeCompositor::hyprland_lua);
    assert_eq!(actions, [format!(r#"dispatch hl.dsp.focus({{ window = "address:{address}" }})"#)], "Lua config");
}

#[test]
fn pins_keep_their_order_and_survive_a_restart() {
    let novad = TestDaemon::with_launcher("launcher-pins");
    for name in ["a", "b", "c"] {
        install_app(&novad, &format!("{name}.desktop"), &format!("Name={name}\nExec=true\n"));
    }
    let mut client = novad.connect();
    client.subscribe_launcher(3);
    for (id, name) in [(2, "c"), (3, "a"), (4, "b")] {
        client.request(id, "pin_app", Some(json!({"id": format!("{name}.desktop"), "pinned": true})));
    }
    client.request(5, "pin_app", Some(json!({"id": "a.desktop", "pinned": false})));
    let event = client.launcher_until(|event| ids(&event["pinned"]) == ["c.desktop", "b.desktop"]);
    assert_eq!(ids(&event["pinned"]).len(), 2);
    let unknown = client.request(6, "pin_app", Some(json!({"id": "../../etc/passwd", "pinned": true})));
    assert!(unknown["error"].as_str().unwrap().contains("no app"));

    // A pinned app that's uninstalled isn't sent, but comes back with it.
    std::fs::remove_file(novad.dir.join("launcher/share/applications/c.desktop")).unwrap();
    client.launcher_until(|event| ids(&event["pinned"]) == ["b.desktop"]);
    install_app(&novad, "c.desktop", "Name=c\nExec=true\n");
    client.launcher_until(|event| ids(&event["pinned"]) == ["c.desktop", "b.desktop"]);

    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(novad.dir.join("launcher/launcher.json")).unwrap()).unwrap();
    assert_eq!(saved["pinned"], json!(["c.desktop", "b.desktop"]));
}

#[test]
fn an_app_is_found_by_its_own_id_before_a_shortcut_running_it() {
    let novad = TestDaemon::with_launcher("find-app");
    // Steam's game shortcuts run `steam`, and sort before it.
    install_app(&novad, "Abiotic Factor.desktop", "Name=Abiotic Factor\nExec=steam steam://rungameid/427410\n");
    install_app(&novad, "steam.desktop", "Name=Steam\nExec=steam %U\n");
    let apps = Arc::clone(&lock(&novad.daemon.settings).context.apps);
    wait_until(Duration::from_secs(5), "the index to find both apps", || {
        apps.find_app(&["abiotic factor".into()]).is_some() && apps.find_app(&["steam".into()]).is_some()
    });
    assert_eq!(apps.find_app(&["steam".into()]).unwrap().id, "steam.desktop");
}

#[test]
fn the_launcher_stops_with_its_feature() {
    let novad = TestDaemon::with_launcher("launcher-off");
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["features"]})));
    client.next_event("features");
    std::fs::write(novad.features_file(), "disabled = [\"ai-usage\", \"launcher\"]\n").unwrap();
    loop {
        let features = client.next_event("features");
        let launcher = features["features"].as_array().unwrap().iter().find(|f| f["id"] == "launcher").cloned();
        if launcher.is_some_and(|launcher| launcher["status"] == "off") {
            break;
        }
    }
    let off = client.request(2, "launcher_search", Some(json!({"query": "a"})));
    assert!(off["error"].as_str().unwrap().contains("feature isn't ready"));
    let toggle = client.request(3, "toggle", Some(json!({"panel": "launcher"})));
    assert!(toggle["error"].as_str().unwrap().contains("isn't ready"));
}

#[test]
fn launching_a_tray_only_app_activates_its_item_then_starts_it_when_no_window_shows() {
    let mut bus = None;
    let mut compositor = None;
    let novad = TestDaemon::open_following(
        "launcher-tray",
        None,
        |dir| {
            std::fs::write(dir.join("config/features.toml"), AI_USAGE_OFF).unwrap();
            bus = Some(nova_session_bus::TestBus::start(&dir.join("bus")));
        },
        |dir| {
            let (fake, backend) = FakeCompositor::niri(dir);
            compositor = Some(fake);
            Some(backend)
        },
    );
    let (bus, compositor) = (bus.unwrap(), compositor.unwrap());
    let marker = novad.dir.join("ran");
    let entry = format!("Name=Third\nExec=touch \"{}\"\nStartupWMClass=Nova-Third\n", marker.display());
    install_app(&novad, "third.desktop", &entry);
    let (mut client, desktop) = follow_to_the_end(&novad);
    client.subscribe_launcher(1);
    // Its icon's id names it; the recording has no window of it.
    let activated = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = bus.connect();
    let item = nova_tray::test_items::Item {
        id: "nova-third".into(),
        title: "Third".into(),
        activated: Arc::clone(&activated),
    };
    nova_tray::test_items::serve_item(&app, None, "/StatusNotifierItem", "/StatusNotifierItem", item);
    client.request(4, "subscribe", Some(json!({"topics": ["tray"]})));
    while client.next_event("tray")["items"][0]["app"]["id"] != "third.desktop" {}
    let output = focused_output(&desktop);

    let launch = json!({"provider": "apps", "id": "third.desktop", "output": output});
    let asked = std::time::Instant::now();
    assert_eq!(client.request(5, "launcher_run", Some(launch))["result"], Value::Null);
    assert!(
        asked.elapsed() < nova_launcher::SHOWN_WINDOW_WAIT,
        "answered once the icon was asked, not after waiting for a window: the launcher closes on the answer"
    );
    assert_eq!(activated.load(std::sync::atomic::Ordering::SeqCst), 1, "the tray icon is asked to show its window");
    assert!(compositor.actions().is_empty(), "no window showed to focus");
    // Like Claude Desktop, whose icon ignores Activate: started again, it shows.
    assert!(ran(&marker), "no window showed, so the app is started");

    std::fs::remove_file(&marker).unwrap();
    let fresh = json!({"provider": "apps", "id": "third.desktop", "output": output, "new_window": true});
    assert_eq!(client.request(6, "launcher_run", Some(fresh))["result"], Value::Null);
    assert!(ran(&marker), "asked for a new window, the app runs");
    assert_eq!(activated.load(std::sync::atomic::Ordering::SeqCst), 1, "a new window skips the tray");
}

/// Whether the app that touches `marker` ran, waiting up to 5 s for it.
fn ran(marker: &std::path::Path) -> bool {
    (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        marker.exists()
    })
}

#[test]
fn a_tray_click_focuses_the_open_window_of_its_app() {
    let mut bus = None;
    let mut compositor = None;
    let novad = TestDaemon::open_following(
        "tray-click-focus",
        None,
        |dir| {
            std::fs::write(dir.join("config/features.toml"), AI_USAGE_OFF).unwrap();
            bus = Some(nova_session_bus::TestBus::start(&dir.join("bus")));
        },
        |dir| {
            let (fake, backend) = FakeCompositor::niri(dir);
            compositor = Some(fake);
            Some(backend)
        },
    );
    let (bus, compositor) = (bus.unwrap(), compositor.unwrap());
    // The recording's window on workspace `web` is this app's.
    let marker = novad.dir.join("ran");
    let entry = format!("Name=Second\nExec=touch \"{}\"\nStartupWMClass=Nova-Second\n", marker.display());
    install_app(&novad, "second.desktop", &entry);
    let (mut client, _desktop) = follow_to_the_end(&novad);
    client.subscribe_launcher(1);
    let activated = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = bus.connect();
    let item = nova_tray::test_items::Item {
        id: "nova-second".into(),
        title: "Second".into(),
        activated: Arc::clone(&activated),
    };
    nova_tray::test_items::serve_item(&app, None, "/StatusNotifierItem", "/StatusNotifierItem", item);
    client.request(2, "subscribe", Some(json!({"topics": ["tray"]})));
    let address = loop {
        let tray = client.next_event("tray");
        if tray["items"][0]["app"]["id"] == "second.desktop" {
            break tray["items"][0]["address"].as_str().unwrap().to_owned();
        }
    };

    let click = client.request(3, "activate_tray_item", Some(json!({"address": address})));
    assert_eq!(click["result"], Value::Null, "{click}");
    assert_eq!(activated.load(std::sync::atomic::Ordering::SeqCst), 1, "the app hears the click");
    let focused = json!({"Action": {"FocusWindow": {"id": 3}}});
    assert_eq!(compositor.actions(), [focused.to_string()], "its open window takes focus where it is");
    assert!(!ran(&marker), "an app with a window open isn't started, even if the click hid it");

    let gone = client.request(4, "activate_tray_item", Some(json!({"address": ":1.999/Nowhere"})));
    assert!(gone["error"].as_str().unwrap().contains("no tray icon"), "{gone}");
}

#[test]
fn a_tray_click_starts_its_app_when_no_window_shows() {
    let mut bus = None;
    let mut compositor = None;
    let novad = TestDaemon::open_following(
        "tray-click-start",
        None,
        |dir| {
            std::fs::write(dir.join("config/features.toml"), AI_USAGE_OFF).unwrap();
            bus = Some(nova_session_bus::TestBus::start(&dir.join("bus")));
        },
        |dir| {
            let (fake, backend) = FakeCompositor::niri(dir);
            compositor = Some(fake);
            Some(backend)
        },
    );
    let (bus, compositor) = (bus.unwrap(), compositor.unwrap());
    let marker = novad.dir.join("ran");
    let entry = format!("Name=Third\nExec=touch \"{}\"\nStartupWMClass=Nova-Third\n", marker.display());
    install_app(&novad, "third.desktop", &entry);
    let (mut client, _desktop) = follow_to_the_end(&novad);
    client.subscribe_launcher(1);
    // Its icon's id names it; the recording has no window of it.
    let activated = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = bus.connect();
    let item = nova_tray::test_items::Item {
        id: "nova-third".into(),
        title: "Third".into(),
        activated: Arc::clone(&activated),
    };
    nova_tray::test_items::serve_item(&app, None, "/StatusNotifierItem", "/StatusNotifierItem", item);
    client.request(2, "subscribe", Some(json!({"topics": ["tray"]})));
    let address = loop {
        let tray = client.next_event("tray");
        if tray["items"][0]["app"]["id"] == "third.desktop" {
            break tray["items"][0]["address"].as_str().unwrap().to_owned();
        }
    };

    let click = client.request(3, "activate_tray_item", Some(json!({"address": address})));
    assert_eq!(click["result"], Value::Null, "{click}");
    assert_eq!(activated.load(std::sync::atomic::Ordering::SeqCst), 1, "the app hears the click");
    assert!(compositor.actions().is_empty(), "no window showed to focus");
    // Like Claude Desktop, whose icon ignores Activate: started again, it shows.
    assert!(ran(&marker), "no window showed, so the app is started");
}

#[test]
fn launching_an_app_whose_tray_icon_has_only_a_menu_starts_it_again_and_focuses_its_window() {
    let mut bus = None;
    let mut compositor = None;
    let novad = TestDaemon::open_following(
        "launcher-tray-menu",
        None,
        |dir| {
            std::fs::write(dir.join("config/features.toml"), AI_USAGE_OFF).unwrap();
            bus = Some(nova_session_bus::TestBus::start(&dir.join("bus")));
        },
        |dir| {
            let (fake, backend) = FakeCompositor::niri(dir);
            compositor = Some(fake);
            Some(backend)
        },
    );
    let (bus, compositor) = (bus.unwrap(), Arc::new(compositor.unwrap()));
    let marker = novad.dir.join("ran");
    let entry = format!("Name=Third\nExec=touch \"{}\"\nStartupWMClass=Nova-Third\n", marker.display());
    install_app(&novad, "third.desktop", &entry);
    let (mut client, desktop) = follow_to_the_end(&novad);
    client.subscribe_launcher(1);
    // Like Spotify's: libayatana's icons have only a menu, no Activate.
    let app = bus.connect();
    let item = nova_tray::test_items::MenuOnlyItem { id: "nova-third".into(), title: "Third".into() };
    nova_tray::test_items::serve_item(
        &app,
        None,
        "/org/ayatana/NotificationItem/third",
        "/org/ayatana/NotificationItem/third",
        item,
    );
    client.request(4, "subscribe", Some(json!({"topics": ["tray"]})));
    while client.next_event("tray")["items"][0]["app"]["id"] != "third.desktop" {}
    // Started again, the running app shows its window, which the compositor
    // doesn't focus.
    let shows = {
        let (marker, compositor) = (marker.clone(), Arc::clone(&compositor));
        std::thread::spawn(move || {
            if ran(&marker) {
                compositor.open_window(9, "nova-third");
            }
        })
    };

    let launch = json!({"provider": "apps", "id": "third.desktop", "output": focused_output(&desktop)});
    assert_eq!(client.request(5, "launcher_run", Some(launch))["result"], Value::Null);
    shows.join().unwrap();
    assert!(marker.exists(), "its icon can't show its window, so the app is started again");
    let focused = json!({"Action": {"FocusWindow": {"id": 9}}});
    // The launcher heard back once the app was started; focusing follows.
    let deadline = std::time::Instant::now() + nova_launcher::STARTED_WINDOW_WAIT;
    while compositor.actions().is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(compositor.actions(), [focused.to_string()], "the window it shows takes focus");
}

#[test]
fn a_window_shown_from_a_tray_menu_takes_focus() {
    let mut bus = None;
    let mut compositor = None;
    let novad = TestDaemon::open_following(
        "tray-menu-focus",
        None,
        |dir| {
            std::fs::write(dir.join("config/features.toml"), AI_USAGE_OFF).unwrap();
            bus = Some(nova_session_bus::TestBus::start(&dir.join("bus")));
        },
        |dir| {
            let (fake, backend) = FakeCompositor::niri(dir);
            compositor = Some(fake);
            Some(backend)
        },
    );
    let (bus, compositor) = (bus.unwrap(), Arc::new(compositor.unwrap()));
    // The recording's window on workspace `web` is Second's; Third has none.
    install_app(&novad, "second.desktop", "Name=Second\nExec=true\nStartupWMClass=Nova-Second\n");
    install_app(&novad, "third.desktop", "Name=Third\nExec=true\nStartupWMClass=Nova-Third\n");
    let (mut client, _desktop) = follow_to_the_end(&novad);
    client.subscribe_launcher(2);
    let app = bus.connect();
    for name in ["second", "third"] {
        let path = format!("/org/ayatana/NotificationItem/{name}");
        let item = nova_tray::test_items::MenuOnlyItem { id: format!("nova-{name}"), title: name.into() };
        nova_tray::test_items::serve_item(&app, None, &path, &path, item);
    }
    client.request(2, "subscribe", Some(json!({"topics": ["tray"]})));
    let address = |items: &Value, app: &str| {
        let items = items.as_array().unwrap();
        items.iter().find(|item| item["app"]["id"] == app).map(|item| item["address"].as_str().unwrap().to_owned())
    };
    let (second, third) = loop {
        let items = client.next_event("tray")["items"].clone();
        if let (Some(second), Some(third)) = (address(&items, "second.desktop"), address(&items, "third.desktop")) {
            break (second, third);
        }
    };

    // "Show Third": novad answers once it has looked, the UI runs the entry,
    // and the window that shows takes focus.
    let picked = client.request(3, "tray_menu_picked", Some(json!({"address": third})));
    assert_eq!(picked["result"], Value::Null, "{picked}");
    compositor.open_window(9, "nova-third");
    let focused = json!({"Action": {"FocusWindow": {"id": 9}}});
    nova_test_support::wait_until(Duration::from_secs(5), "the shown window to take focus", || {
        !compositor.actions().is_empty()
    });
    assert_eq!(compositor.actions(), [focused.to_string()]);

    // Any entry of an app with a window open, like pausing a song, leaves it be.
    let picked = client.request(4, "tray_menu_picked", Some(json!({"address": second})));
    assert_eq!(picked["result"], Value::Null, "{picked}");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(compositor.actions(), [focused.to_string()], "no other window took focus");
}

/// Sends a notification from `app` (on `connection`) whose desktop entry is
/// `entry`, with a default action and a button `reply`; returns its id.
fn notify(connection: &zbus::Connection, app: &str, entry: &str) -> u32 {
    let hints = std::collections::HashMap::from([("desktop-entry", zbus::Variant::from(entry))]);
    let sent = (app, 0u32, "", "Mara", "hi", vec!["default", "Open", "reply", "Reply"], hints, -1i32);
    let path = "/org/freedesktop/Notifications";
    let reply = zbus::block_on(connection.call_method(
        Some(nova_notifications::BUS_NAME),
        path,
        Some(nova_notifications::BUS_NAME),
        "Notify",
        &sent,
    ))
    .unwrap();
    reply.body().deserialize().unwrap()
}

#[test]
fn a_notification_click_focuses_the_window_its_app_shows_or_opens_an_app_that_has_gone() {
    let mut bus = None;
    let mut compositor = None;
    let novad = TestDaemon::open_following(
        "notification-click",
        None,
        |dir| {
            std::fs::write(dir.join("config/features.toml"), AI_USAGE_OFF).unwrap();
            bus = Some(nova_session_bus::TestBus::start(&dir.join("bus")));
        },
        |dir| {
            let (fake, backend) = FakeCompositor::niri(dir);
            compositor = Some(fake);
            Some(backend)
        },
    );
    let (bus, compositor) = (bus.unwrap(), Arc::new(compositor.unwrap()));
    // The recording has no window of either app.
    let (third, fourth) = (novad.dir.join("third-ran"), novad.dir.join("fourth-ran"));
    for (file, name, marker) in [("third.desktop", "Third", &third), ("fourth.desktop", "Fourth", &fourth)] {
        let entry = format!("Name={name}\nExec=touch \"{}\"\nStartupWMClass=Nova-{name}\n", marker.display());
        install_app(&novad, file, &entry);
    }
    let (mut client, desktop) = follow_to_the_end(&novad);
    client.subscribe_launcher(2);
    let output = focused_output(&desktop);
    let app = bus.connect();
    let (button, clicked) = (notify(&app, "Third", "third"), notify(&app, "Third", "third"));
    // Like Ferdium: shows its window when told of the click, which niri
    // doesn't focus.
    let rule =
        zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).member("ActionInvoked").build().unwrap();
    let mut invoked = zbus::block_on(zbus::MessageStream::for_match_rule(rule, &app, None)).unwrap();
    let shows = {
        let compositor = Arc::clone(&compositor);
        std::thread::spawn(move || {
            loop {
                use futures_core::Stream;
                let next = std::future::poll_fn(|cx| std::pin::Pin::new(&mut invoked).poll_next(cx));
                let message = zbus::block_on(next).unwrap().unwrap();
                let (id, key): (u32, String) = message.body().deserialize().unwrap();
                if key == "default" {
                    assert_eq!(id, clicked);
                    compositor.open_window(9, "nova-third");
                    return;
                }
            }
        })
    };

    let reply = client.request(2, "notification_action", Some(json!({"id": button, "action": "reply"})));
    assert_eq!(reply["result"], Value::Null, "{reply}");
    let click = client.request(3, "open_notification", Some(json!({"id": clicked, "output": output})));
    assert_eq!(click["result"], Value::Null, "{click}");
    shows.join().unwrap();
    let focused = json!({"Action": {"FocusWindow": {"id": 9}}});
    assert_eq!(compositor.actions(), [focused.to_string()], "the window it shows takes focus; a button moves none");
    assert!(!ran(&third), "the app took the click, so it isn't started");

    // Like notify-send: its sender is gone by the time it's clicked.
    let gone = bus.connect();
    let dead = notify(&gone, "Fourth", "fourth");
    drop(gone);
    client.request(4, "subscribe", Some(json!({"topics": ["notifications"]})));
    loop {
        let list = client.next_event("notifications");
        let items = list["items"].as_array().unwrap();
        if items.iter().any(|item| item["id"] == dead && item.get("live").is_none()) {
            break;
        }
    }
    let click = client.request(5, "open_notification", Some(json!({"id": dead, "output": output})));
    assert_eq!(click["result"], Value::Null, "{click}");
    assert!(ran(&fourth), "an app that can't hear of the click is opened instead");
}

/// The app index serves whatever names apps, not only the launcher: with
/// the launcher off, an app stream still gets its app's icon while the
/// tray, notifications, media or Nova Files run, and the apps are forgotten
/// once none of them does.
#[test]
fn apps_are_named_with_the_launcher_off() {
    let novad = TestDaemon::start("apps-without-launcher", None);
    // The icon's name isn't the program's, so only the app's entry finds it.
    let claude = install_icon(&novad, "claude-logo");
    install_app(&novad, "claude.desktop", "Name=Claude\nExec=claude-desktop\nIcon=claude-logo\n");
    let mut client = novad.connect();
    let mut id = 0;
    let mut icon = |client: &mut Client| {
        id += 1;
        let args = json!({"icon_name": "", "program": "/usr/bin/claude-desktop", "name": "Claude"});
        client.request(id, "app_icon", Some(args))["result"].clone()
    };
    wait_until(Duration::from_secs(5), "the app's icon without the launcher", || icon(&mut client) == json!(claude));

    std::fs::write(
        novad.features_file(),
        "disabled = [\"ai-usage\", \"files\", \"launcher\", \"tray\", \"notifications\", \"media\"]\n",
    )
    .unwrap();
    wait_until(Duration::from_secs(5), "the apps to be forgotten", || icon(&mut client) == json!(null));
}
