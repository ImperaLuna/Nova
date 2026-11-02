//! Compositor IPC servers replaying recorded niri and Hyprland sessions.

use super::*;

/// A compositor's sockets in a test's folder, serving what the real one sent
/// in `compositor/recorded/<name>`: its whole event stream at once and, for
/// Hyprland, its answers at the end of the recording. Every request is kept.
pub(super) struct FakeCompositor {
    requests: Arc<Mutex<Vec<String>>>,
    /// The windows the fake niri lists, as JSON (the recording's at first).
    windows: Arc<Mutex<Vec<Value>>>,
}

impl FakeCompositor {
    pub(super) fn recorded(compositor: &str, file: &str) -> String {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/compositor/recorded").join(compositor);
        std::fs::read_to_string(dir.join(file)).unwrap()
    }

    /// Answers each connection's one request with `respond`, then keeps the
    /// connection open until the client closes it, as an event stream is.
    pub(super) fn answer(&self, socket: &Path, respond: impl Fn(&str) -> String + Send + Sync + 'static) {
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        let (requests, respond) = (Arc::clone(&self.requests), Arc::new(respond));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let (requests, respond) = (Arc::clone(&requests), Arc::clone(&respond));
                std::thread::spawn(move || {
                    let mut stream = stream.unwrap();
                    // niri's requests end with a newline; Hyprland's clients stop writing.
                    let mut request = String::new();
                    BufReader::new(&stream).read_line(&mut request).unwrap();
                    let request = request.trim().to_owned();
                    let reply = respond(&request);
                    lock(&requests).push(request);
                    stream.write_all(reply.as_bytes()).unwrap();
                    let _ = std::io::copy(&mut stream, &mut std::io::sink());
                });
            }
        });
    }

    /// Sends `events` to each client that connects, without waiting for a
    /// request, and keeps the connection open.
    pub(super) fn stream(socket: &Path, events: String) {
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                stream.write_all(events.as_bytes()).unwrap();
                std::thread::spawn(move || std::io::copy(&mut stream, &mut std::io::sink()));
            }
        });
    }

    pub(super) fn niri(dir: &Path) -> (Self, Box<dyn compositor::Backend>) {
        let steps: Vec<Value> = serde_json::from_str(&Self::recorded("niri", "steps.json")).unwrap();
        let windows = Self::recorded("niri", &format!("{:02}/windows.json", steps.len() - 1));
        let fake =
            Self { requests: Arc::default(), windows: Arc::new(Mutex::new(serde_json::from_str(&windows).unwrap())) };
        let events = Self::recorded("niri", "events.txt");
        let windows = Arc::clone(&fake.windows);
        let outputs = Self::recorded("niri", &format!("{:02}/outputs.json", steps.len() - 1));
        let outputs = format!("{}\n", json!({"Ok": {"Outputs": serde_json::from_str::<Value>(&outputs).unwrap()}}));
        let socket = dir.join("niri.sock");
        fake.answer(&socket, move |request| match request {
            // The recording starts with niri's answer to this.
            r#""EventStream""# => events.clone(),
            r#""Windows""# => format!("{}\n", json!({"Ok": {"Windows": *lock(&windows)}})),
            r#""Outputs""# => outputs.clone(),
            _ => "{\"Ok\":\"Handled\"}\n".into(),
        });
        let backend = compositor::detect(|name| (name == "NIRI_SOCKET").then(|| socket.clone().into())).unwrap();
        (fake, backend)
    }

    /// Hyprland on a `hyprland.conf`, as the recording's was: it knows no
    /// Lua dispatchers.
    pub(super) fn hyprland(dir: &Path) -> (Self, Box<dyn compositor::Backend>) {
        Self::hyprland_on(
            dir,
            |dispatcher| {
                if dispatcher.starts_with("hl.") { "Invalid dispatcher".into() } else { "ok".into() }
            },
        )
    }

    /// Hyprland on a `hyprland.lua` (all of 0.57), answering as the real one
    /// in `recorded/hyprland-0.57/dispatch.json` did: `dispatched` is what it
    /// said to a dispatcher it ran, `built` what building one said
    /// (`scripts/record-hyprland-dispatch.py`): a dispatcher is `userdata`.
    /// One that built runs and gets the answer `hl.dsp.no_op()` got; one that
    /// didn't gets its error.
    pub(super) fn hyprland_lua(dir: &Path) -> (Self, Box<dyn compositor::Backend>) {
        let recorded: Value = serde_json::from_str(&Self::recorded("hyprland-0.57", "dispatch.json")).unwrap();
        Self::hyprland_on(dir, move |dispatcher| {
            if let Some(answer) = recorded["dispatched"][dispatcher].as_str() {
                return answer.to_owned();
            }
            let Some(built) = recorded["built"][dispatcher].as_str() else {
                panic!("record {dispatcher:?} in hyprland-0.57/dispatch.json with scripts/record-hyprland-dispatch.py")
            };
            if built.trim() == "userdata" {
                recorded["dispatched"]["hl.dsp.no_op()"].as_str().unwrap().to_owned()
            } else {
                built.to_owned()
            }
        })
    }

    /// The recorded Hyprland, answering each dispatcher with `dispatched`.
    fn hyprland_on(
        dir: &Path,
        dispatched: impl Fn(&str) -> String + Send + Sync + 'static,
    ) -> (Self, Box<dyn compositor::Backend>) {
        const INSTANCE: &str = "recorded";
        let fake = Self { requests: Arc::default(), windows: Arc::default() };
        let instance = dir.join("hypr").join(INSTANCE);
        std::fs::create_dir_all(&instance).unwrap();
        let steps: Vec<Value> = serde_json::from_str(&Self::recorded("hyprland", "steps.json")).unwrap();
        let last = format!("{:02}", steps.len() - 1);
        let events = Self::recorded("hyprland", "events.txt");
        Self::stream(&instance.join(".socket2.sock"), events);
        fake.answer(&instance.join(".socket.sock"), move |request| {
            if let Some(query) = request.strip_prefix("j/") {
                return Self::recorded("hyprland", &format!("{last}/{query}.json"));
            }
            request.strip_prefix("dispatch ").map_or_else(|| "ok".into(), &dispatched)
        });
        let runtime_dir = dir.to_owned();
        let backend = compositor::detect(|name| match name {
            "HYPRLAND_INSTANCE_SIGNATURE" => Some(INSTANCE.into()),
            "XDG_RUNTIME_DIR" => Some(runtime_dir.clone().into()),
            _ => None,
        })
        .unwrap();
        (fake, backend)
    }

    /// Has the fake niri list a window of `app_id`, with id `id`, on the
    /// recording's workspace 2, as an app that shows one does.
    pub(super) fn open_window(&self, id: u64, app_id: &str) {
        let window = json!({"id": id, "title": app_id, "app_id": app_id, "pid": 2000, "workspace_id": 2,
            "is_focused": false, "is_floating": false, "is_urgent": false});
        lock(&self.windows).push(window);
    }

    /// The requests that weren't queries or the event stream.
    pub(super) fn actions(&self) -> Vec<String> {
        let queries = [r#""EventStream""#, r#""Windows""#, r#""Outputs""#, compositor::HYPRLAND_LUA_PROBE];
        lock(&self.requests)
            .iter()
            .filter(|r| !r.starts_with("j/") && !queries.contains(&r.as_str()))
            .cloned()
            .collect()
    }

    /// How many requests were exactly `request`.
    pub(super) fn sent(&self, request: &str) -> usize {
        lock(&self.requests).iter().filter(|r| *r == request).count()
    }
}

/// novad follows `fake` until its desktop has what the recording ends with:
/// the second layout, no focused window, and a workspace named `web`
/// (niri) or Hyprland's special workspace `magic`, which the window on `web`
/// moved to.
pub(super) fn follow_to_the_end(novad: &TestDaemon) -> (Client, Value) {
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["compositor"]})));
    loop {
        let desktop = client.next_event("compositor");
        let names: Vec<&str> = desktop["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|output| output["workspaces"].as_array().unwrap())
            .map(|workspace| workspace["name"].as_str().unwrap())
            .collect();
        if desktop["keyboard_layout"] == "German"
            && (names.contains(&"web") || names.contains(&"magic"))
            && desktop.get("focused_window").is_none()
        {
            return (client, desktop);
        }
    }
}

pub(super) fn workspace_id(desktop: &Value, name: &str) -> i64 {
    desktop["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["workspaces"].as_array().unwrap())
        .find(|workspace| workspace["name"] == name)
        .and_then(|workspace| workspace["id"].as_i64())
        .unwrap()
}

pub(super) fn focused_output(desktop: &Value) -> String {
    let outputs = desktop["outputs"].as_array().unwrap();
    outputs.iter().find(|output| output["focused"] == true).unwrap()["name"].as_str().unwrap().to_owned()
}
