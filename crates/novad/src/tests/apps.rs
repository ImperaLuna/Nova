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
    install_app(&novad, "foot.desktop", "Name=Foot\nGenericName=Terminal\nIcon=foot\nExec=foot\n");
    install_app(&novad, "hidden.desktop", "Name=Hidden\nNoDisplay=true\nExec=x\n");
    let mut client = novad.connect();
    let first = client.subscribe_launcher(1);
    assert_eq!(
        first["apps"],
        json!([{"id": "foot.desktop", "name": "Foot", "description": "Terminal", "icon": "foot"}])
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

    let found = client.request(2, "search_apps", Some(json!({"query": " FI "})));
    // Fooview has f and i in order, which counts least.
    assert_eq!(ids(&found["result"]), ["files.desktop", "firefox.desktop", "fooview.desktop"]);
    assert_eq!(found["result"][0]["name"], json!([{"text": "Fi", "matched": true}, {"text": "les"}]));
    let by_keyword = client.request(3, "search_apps", Some(json!({"query": "folder"})));
    assert_eq!(by_keyword["result"], json!([{"id": "files.desktop", "name": [{"text": "Files"}]}]));
    assert_eq!(client.request(4, "search_apps", Some(json!({"query": ""})))["result"], json!([]));
    assert_eq!(client.request(5, "search_apps", Some(json!({"query": "zzz"})))["result"], json!([]));
    let long = client.request(6, "search_apps", Some(json!({"query": "x".repeat(201)})));
    assert!(long["error"].as_str().unwrap().contains("at most 200"));
}

#[test]
fn app_streams_get_their_apps_icons_by_program_and_name() {
    let novad = TestDaemon::with_launcher("app-icon");
    install_app(&novad, "zen-beta.desktop", "Name=Zen Browser\nExec=zen-beta\nIcon=zen-browser\n");
    install_app(&novad, "com.anthropic.Claude.desktop", "Name=Claude\nExec=claude-desktop\nIcon=claude\n");
    install_app(&novad, "firefox.desktop", "Name=Firefox\nExec=firefox\nIcon=firefox-icon\n");
    let mut client = novad.connect();
    client.subscribe_launcher(3);
    let icon = |client: &mut Client, id, [icon_name, program, name]: [&str; 3]| {
        client.request(id, "app_icon", Some(json!({"icon_name": icon_name, "program": program, "name": name})))
    };

    // Found loosely: Zen plays as `.zen-wrapped`, its entry is "Zen Browser".
    assert_eq!(icon(&mut client, 2, ["", "/nix/store/x-zen/lib/zen/.zen-wrapped", "Zen"])["result"], "zen-browser");
    // By the icon name the stream gives; Electron says nothing.
    assert_eq!(
        icon(&mut client, 3, ["com.anthropic.Claude", "/usr/lib/electron/electron", "Claude"])["result"],
        "claude"
    );
    assert_eq!(icon(&mut client, 4, ["", "/usr/bin/firefox", "Firefox"])["result"], "firefox-icon");
    // A browser's web app (`zen-<id>.desktop`) isn't the browser.
    install_app(&novad, "zen-abc123-Default.desktop", "Name=YouTube\nExec=zen-beta --app=abc123\nIcon=youtube\n");
    client.launcher_until(|event| event["apps"].as_array().unwrap().len() == 4);
    assert_eq!(icon(&mut client, 5, ["", "/nix/store/x-zen/lib/zen/.zen-wrapped", "Zen"])["result"], "zen-browser");
    // No app: the icon name it gives, else its program, for the icon theme.
    assert_eq!(icon(&mut client, 6, ["", "/usr/bin/mpv", "mpv"])["result"], "mpv");
    assert_eq!(icon(&mut client, 7, ["audio-x-generic", "/usr/bin/mpv", "mpv"])["result"], "audio-x-generic");
    assert_eq!(icon(&mut client, 8, ["", "", "Some Game"])["result"], json!(null));
    let long = icon(&mut client, 9, ["", &"a".repeat(513), ""]);
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

    assert_eq!(client.request(2, "launch_app", Some(json!({"id": "touch.desktop"})))["result"], Value::Null);
    let event = client.launcher_until(|event| !event["recent"].as_array().unwrap().is_empty());
    assert_eq!(event["recent"], json!(["touch.desktop"]));
    let ran = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        marker.exists()
    });
    assert!(ran, "the app didn't run");
    assert!(novad.dir.join(";").exists(), "`;` reached touch as a file name, not a shell");

    // No Path= and a HOME that may not exist (Nix's builds): it starts in `/`.
    assert_eq!(client.request(3, "launch_app", Some(json!({"id": "other.desktop"})))["result"], Value::Null);
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
    let missing = client.request(7, "launch_app", Some(json!({"id": "nothing.desktop"})));
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
    assert_eq!(client.request(2, "launch_app", Some(json!({"id": "doomed.desktop"})))["result"], Value::Null);
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

/// Launches the app the recording's window belongs to (`nova-second`, on
/// workspace `web`) from the focused output, and returns the actions taken
/// and whether the app ran anyway, then the same asking for a new window.
fn focus_the_recorded_window(
    name: &str,
    fake: fn(&Path) -> (FakeCompositor, Box<dyn compositor::Backend>),
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
    let launch = json!({"id": "second.desktop", "output": output});
    assert_eq!(client.request(2, "launch_app", Some(launch))["result"], Value::Null);
    let event = client.launcher_until(|event| !event["recent"].as_array().unwrap().is_empty());
    assert_eq!(event["recent"], json!(["second.desktop"]), "focusing it counts as a launch");
    let actions = compositor.actions();

    let fresh = json!({"id": "second.desktop", "output": output, "new_window": true});
    assert_eq!(client.request(3, "launch_app", Some(fresh))["result"], Value::Null);
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

    let again = launcher::Launcher::start(
        launcher_paths(&novad.dir),
        "",
        Arc::new(Hub::default()),
        Arc::clone(&novad.daemon.logger),
    );
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(launcher_paths(&novad.dir).state_file).unwrap()).unwrap();
    assert_eq!(saved["pinned"], json!(["c.desktop", "b.desktop"]));
    again.stop();
}

#[test]
fn an_app_is_found_by_its_own_id_before_a_shortcut_running_it() {
    let novad = TestDaemon::with_launcher("find-app");
    // Steam's game shortcuts run `steam`, and sort before it.
    install_app(&novad, "Abiotic Factor.desktop", "Name=Abiotic Factor\nExec=steam steam://rungameid/427410\n");
    install_app(&novad, "steam.desktop", "Name=Steam\nExec=steam %U\n");
    let launcher = launcher::Launcher::start(
        launcher_paths(&novad.dir),
        "",
        Arc::new(Hub::default()),
        Arc::clone(&novad.daemon.logger),
    );
    wait_until(Duration::from_secs(5), "the launcher to find both apps", || {
        launcher.find_app(&["abiotic factor".into()]).is_some() && launcher.find_app(&["steam".into()]).is_some()
    });
    assert_eq!(launcher.find_app(&["steam".into()]).unwrap().id, "steam.desktop");
    launcher.stop();
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
    let off = client.request(2, "search_apps", Some(json!({"query": "a"})));
    assert!(off["error"].as_str().unwrap().contains("feature isn't ready"));
    let toggle = client.request(3, "toggle", Some(json!({"panel": "launcher"})));
    assert!(toggle["error"].as_str().unwrap().contains("isn't ready"));
}

#[test]
fn launching_an_app_that_only_sits_in_the_tray_activates_its_item() {
    let mut bus = None;
    let mut compositor = None;
    let novad = TestDaemon::open_following(
        "launcher-tray",
        None,
        |dir| {
            std::fs::write(dir.join("config/features.toml"), AI_USAGE_OFF).unwrap();
            bus = Some(session_bus::TestBus::start(&dir.join("bus")));
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
    let item = tray::tests::Item { id: "nova-third".into(), title: "Third".into(), activated: Arc::clone(&activated) };
    tray::tests::serve_item(&app, None, "/StatusNotifierItem", "/StatusNotifierItem", item);
    client.request(4, "subscribe", Some(json!({"topics": ["tray"]})));
    while client.next_event("tray")["items"][0]["app"]["id"] != "third.desktop" {}
    let output = focused_output(&desktop);

    let launch = json!({"id": "third.desktop", "output": output});
    assert_eq!(client.request(5, "launch_app", Some(launch))["result"], Value::Null);
    assert_eq!(activated.load(std::sync::atomic::Ordering::SeqCst), 1, "the tray icon shows its window");
    std::thread::sleep(Duration::from_millis(200));
    assert!(!marker.exists(), "starting it again would just quit");
    assert!(compositor.actions().is_empty(), "no window showed to focus");

    let fresh = json!({"id": "third.desktop", "output": output, "new_window": true});
    assert_eq!(client.request(6, "launch_app", Some(fresh))["result"], Value::Null);
    let ran = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        marker.exists()
    });
    assert!(ran, "asked for a new window, the app runs");
    assert_eq!(activated.load(std::sync::atomic::Ordering::SeqCst), 1);
}
