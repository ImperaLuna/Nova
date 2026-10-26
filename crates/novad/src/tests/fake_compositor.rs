//! Compositor IPC servers replaying recorded niri and Hyprland sessions.

use super::*;

/// A compositor's sockets in a test's folder, serving what the real one sent
/// in `compositor/recorded/<name>`: its whole event stream at once and, for
/// Hyprland, its answers at the end of the recording. Every request is kept.
pub(super) struct FakeCompositor {
    requests: Arc<Mutex<Vec<String>>>,
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
        let fake = Self { requests: Arc::default() };
        let events = Self::recorded("niri", "events.txt");
        let steps: Vec<Value> = serde_json::from_str(&Self::recorded("niri", "steps.json")).unwrap();
        let windows = Self::recorded("niri", &format!("{:02}/windows.json", steps.len() - 1));
        let outputs = Self::recorded("niri", &format!("{:02}/outputs.json", steps.len() - 1));
        let outputs = format!("{}\n", json!({"Ok": {"Outputs": serde_json::from_str::<Value>(&outputs).unwrap()}}));
        let socket = dir.join("niri.sock");
        fake.answer(&socket, move |request| match request {
            // The recording starts with niri's answer to this.
            r#""EventStream""# => events.clone(),
            r#""Windows""# => format!("{{\"Ok\":{{\"Windows\":{}}}}}\n", windows.trim()),
            r#""Outputs""# => outputs.clone(),
            _ => "{\"Ok\":\"Handled\"}\n".into(),
        });
        let backend = compositor::detect(|name| (name == "NIRI_SOCKET").then(|| socket.clone().into())).unwrap();
        (fake, backend)
    }

    pub(super) fn hyprland(dir: &Path) -> (Self, Box<dyn compositor::Backend>) {
        const INSTANCE: &str = "recorded";
        let fake = Self { requests: Arc::default() };
        let instance = dir.join("hypr").join(INSTANCE);
        std::fs::create_dir_all(&instance).unwrap();
        let steps: Vec<Value> = serde_json::from_str(&Self::recorded("hyprland", "steps.json")).unwrap();
        let last = format!("{:02}", steps.len() - 1);
        let events = Self::recorded("hyprland", "events.txt");
        Self::stream(&instance.join(".socket2.sock"), events);
        fake.answer(&instance.join(".socket.sock"), move |request| match request.strip_prefix("j/") {
            Some(query) => Self::recorded("hyprland", &format!("{last}/{query}.json")),
            None => "ok".into(),
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

    /// The requests that weren't queries or the event stream.
    pub(super) fn actions(&self) -> Vec<String> {
        let queries = [r#""EventStream""#, r#""Windows""#, r#""Outputs""#];
        lock(&self.requests)
            .iter()
            .filter(|r| !r.starts_with("j/") && !queries.contains(&r.as_str()))
            .cloned()
            .collect()
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
