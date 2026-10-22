//! `novad`: Nova's background daemon.
//!
//! For now it serves the protocol skeleton: a `hello` and a `features` event
//! on connect, and replies to `ping` and `list_features`. Compositor adapters
//! and services get added on top of this.

use nova_features::{Manifest, Selection};
use nova_protocol::{Command, Envelope, Event, FeatureState, FeatureStatus, Outcome, Request, Response, encode};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Config {
    socket: PathBuf,
    features_dir: PathBuf,
    selection_file: PathBuf,
}

struct State {
    features: Vec<FeatureState>,
}

fn main() {
    let config = match parse_args(std::env::args().skip(1)) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("novad: {message}");
            eprintln!("usage: novad [--socket PATH] [--features-dir DIR] [--selection FILE]");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(&config) {
        eprintln!("novad: {e}");
        std::process::exit(1);
    }
}

fn run(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let manifests = nova_features::load_all(&config.features_dir)?;
    let selection = match std::fs::read_to_string(&config.selection_file) {
        Ok(text) => Selection::parse(&text)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Selection::default(),
        Err(e) => return Err(e.into()),
    };
    let path_var = std::env::var("PATH").unwrap_or_default();
    let state = Arc::new(State { features: feature_states(&manifests, &selection, &path_var) });
    for f in &state.features {
        eprintln!("novad: feature {} is {:?}", f.id, f.status);
    }

    let listener = bind(&config.socket)?;
    eprintln!("novad: listening on {}", config.socket.display());
    serve(listener, state);
    Ok(())
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Config, String> {
    let mut config = Config {
        socket: nova_paths::socket(),
        features_dir: nova_features::default_dir(),
        selection_file: nova_paths::config_dir().join("features.toml"),
    };
    while let Some(arg) = args.next() {
        let mut value = || args.next().map(PathBuf::from).ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--socket" => config.socket = value()?,
            "--features-dir" => config.features_dir = value()?,
            "--selection" => config.selection_file = value()?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(config)
}

fn feature_states(manifests: &[Manifest], selection: &Selection, path_var: &str) -> Vec<FeatureState> {
    manifests
        .iter()
        .map(|m| {
            if !selection.is_on(m) {
                return FeatureState { id: m.id.clone(), status: FeatureStatus::Off, missing: vec![] };
            }
            let missing = m.missing_binaries(path_var);
            let status = if missing.is_empty() { FeatureStatus::Ready } else { FeatureStatus::MissingDeps };
            FeatureState { id: m.id.clone(), status, missing }
        })
        .collect()
}

fn bind(socket: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    // A socket file left by a previous run would make bind fail.
    if UnixStream::connect(socket).is_err() {
        let _ = std::fs::remove_file(socket);
    }
    UnixListener::bind(socket)
}

fn serve(listener: UnixListener, state: Arc<State>) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    if let Err(e) = handle_client(stream, &state) {
                        eprintln!("novad: client error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("novad: accept failed: {e}"),
        }
    }
}

fn handle_client(stream: UnixStream, state: &State) -> std::io::Result<()> {
    let mut writer = stream.try_clone()?;
    writer.write_all(encode(Event::Hello { daemon: env!("CARGO_PKG_VERSION").into() })?.as_bytes())?;
    writer.write_all(encode(Event::Features { features: state.features.clone() })?.as_bytes())?;
    for line in BufReader::new(stream).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle_line(&line, state) {
            writer.write_all(reply.as_bytes())?;
        }
    }
    Ok(())
}

/// Answers one request line. Returns `None` when no reply is due.
fn handle_line(line: &str, state: &State) -> Option<String> {
    let request = match serde_json::from_str::<Envelope<Request>>(line) {
        Ok(request) => request.body,
        Err(e) => {
            eprintln!("novad: ignoring bad request {line:?}: {e}");
            let id = serde_json::from_str::<serde_json::Value>(line).ok()?.get("id")?.as_u64()?;
            return encode(Response { id, outcome: Outcome::Error(format!("bad request: {e}")) }).ok();
        }
    };
    let outcome = match request.command {
        Command::Ping => Outcome::Result("pong".into()),
        Command::ListFeatures => match serde_json::to_value(&state.features) {
            Ok(value) => Outcome::Result(value),
            Err(e) => Outcome::Error(e.to_string()),
        },
    };
    encode(Response { id: request.id?, outcome }).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn state() -> State {
        State { features: vec![FeatureState { id: "core".into(), status: FeatureStatus::Ready, missing: vec![] }] }
    }

    fn parse(line: &str) -> Value {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn ping_gets_pong() {
        let reply = handle_line(r#"{"v":1,"id":1,"cmd":"ping"}"#, &state()).unwrap();
        assert_eq!(parse(&reply), json!({"v": 1, "id": 1, "result": "pong"}));
    }

    #[test]
    fn requests_without_id_get_no_reply() {
        assert!(handle_line(r#"{"v":1,"cmd":"ping"}"#, &state()).is_none());
    }

    #[test]
    fn bad_requests_with_an_id_get_an_error() {
        let reply = handle_line(r#"{"v":1,"id":4,"cmd":"reboot"}"#, &state()).unwrap();
        assert!(parse(&reply)["error"].as_str().unwrap().starts_with("bad request"));
        assert!(handle_line("not json", &state()).is_none());
    }

    #[test]
    fn feature_states_follow_selection_and_path() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        let manifests = nova_features::load_all(&root).unwrap();
        let on = Selection::parse("enabled = [\"recording\"]").unwrap();
        let states = feature_states(&manifests, &on, "/nonexistent");
        let rec = states.iter().find(|s| s.id == "recording").unwrap();
        assert_eq!(rec.status, FeatureStatus::MissingDeps);
        let off = feature_states(&manifests, &Selection::default(), "/nonexistent");
        assert_eq!(off.iter().find(|s| s.id == "recording").unwrap().status, FeatureStatus::Off);
    }

    #[test]
    fn manifest_daemon_names_match_cargo_features() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let cargo: toml::Table = std::fs::read_to_string(root.join("Cargo.toml")).unwrap().parse().unwrap();
        let mut cargo_features: Vec<String> = cargo
            .get("features")
            .and_then(|f| f.as_table())
            .map(|t| t.keys().filter(|k| *k != "default").cloned().collect())
            .unwrap_or_default();
        let mut declared: Vec<String> = nova_features::load_all(&root.join("../../features"))
            .unwrap()
            .into_iter()
            .filter_map(|m| m.daemon)
            .collect();
        cargo_features.sort();
        declared.sort();
        assert_eq!(cargo_features, declared, "novad [features] and manifest `daemon` fields differ");
    }

    #[test]
    fn socket_round_trip() {
        let dir = std::env::temp_dir().join(format!("novad-test-{}", std::process::id()));
        let socket = dir.join("novad.sock");
        let listener = bind(&socket).unwrap();
        std::thread::spawn(move || serve(listener, Arc::new(state())));

        let stream = UnixStream::connect(&socket).unwrap();
        let mut writer = stream.try_clone().unwrap();
        let mut lines = BufReader::new(stream).lines();
        assert_eq!(parse(&lines.next().unwrap().unwrap())["event"], "hello");
        assert_eq!(parse(&lines.next().unwrap().unwrap())["event"], "features");
        writer.write_all(b"{\"v\":1,\"id\":9,\"cmd\":\"ping\"}\n").unwrap();
        assert_eq!(parse(&lines.next().unwrap().unwrap())["result"], "pong");
        let _ = std::fs::remove_dir_all(dir);
    }
}
