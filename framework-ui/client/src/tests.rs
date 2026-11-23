use crate::fake::{FakeDaemon, Recorded, VERSION};
use crate::link::{Incoming, Link, Refusal};
use nova_carrier::Topic;
use nova_daemon_info::Command;
use nova_test_support::TempDir;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(5);
const WEATHER: Topic = Topic::named("weather");
const TIDE: Topic = Topic::named("tide");

fn recorded() -> Recorded {
    let weather = r#"{"event":"weather","data":{"sky":"rain"}}
{"event":"weather","data":{"sky":"sun"}}

{"event":"storm","data":null}
"#;
    let mut recorded = Recorded::new().topic(WEATHER.name(), weather).unwrap();
    recorded = recorded.topic(TIDE.name(), r#"{"event":"tide","data":{"high":true}}"#).unwrap();
    recorded.pace = Duration::from_millis(10);
    recorded
}

fn link(socket: &Path) -> (Link, Receiver<Incoming>) {
    let (sender, incoming) = mpsc::channel();
    let link = Link::open(socket.to_path_buf(), move |message| {
        let _ = sender.send(message);
    });
    (link, incoming)
}

fn next(incoming: &Receiver<Incoming>) -> Incoming {
    incoming.recv_timeout(WAIT).expect("the link said nothing")
}

fn event(name: &str, data: Value) -> Incoming {
    Incoming::Event { name: name.into(), data }
}

fn connected() -> Incoming {
    Incoming::Connected { daemon: VERSION.into() }
}

fn answer(link: &Link, command: Command) -> Result<Value, Refusal> {
    let (sender, answer) = mpsc::channel();
    link.request(command, move |result| {
        let _ = sender.send(result);
    });
    answer.recv_timeout(WAIT).expect("the request got no answer")
}

fn socket(dir: &TempDir) -> PathBuf {
    dir.path().join("novad.sock")
}

#[test]
fn a_topic_sends_its_state_then_its_changes() {
    let dir = TempDir::new("client-topic");
    let _novad = FakeDaemon::start(&socket(&dir), recorded()).unwrap();
    let (link, incoming) = link(&socket(&dir));
    link.subscribe(WEATHER);
    assert_eq!(next(&incoming), connected());
    assert_eq!(next(&incoming), event("weather", json!({"sky": "rain"})));
    assert_eq!(next(&incoming), event("weather", json!({"sky": "sun"})));
    assert_eq!(next(&incoming), event("storm", Value::Null));
    link.subscribe(TIDE);
    assert_eq!(next(&incoming), event("tide", json!({"high": true})));
    assert!(incoming.recv_timeout(Duration::from_millis(100)).is_err());
}

#[test]
fn a_request_gets_its_answer() {
    let dir = TempDir::new("client-request");
    let _novad = FakeDaemon::start(&socket(&dir), recorded()).unwrap();
    let (link, incoming) = link(&socket(&dir));
    assert_eq!(next(&incoming), connected());
    assert_eq!(answer(&link, Command::Ping), Ok("pong".into()));
    let refused = answer(&link, Command::Status);
    assert!(matches!(&refused, Err(Refusal::Daemon(error)) if error.contains("only replays")), "{refused:?}");
}

#[test]
fn a_request_without_novad_is_refused_at_once() {
    let dir = TempDir::new("client-alone");
    let (link, _incoming) = link(&socket(&dir));
    assert_eq!(answer(&link, Command::Ping), Err(Refusal::NotConnected));
}

#[test]
fn it_connects_again_when_novad_restarts() {
    let dir = TempDir::new("client-restart");
    let (link, incoming) = link(&socket(&dir));
    link.subscribe(TIDE);
    for _ in 0..2 {
        let novad = FakeDaemon::start(&socket(&dir), recorded()).unwrap();
        assert_eq!(next(&incoming), connected());
        assert_eq!(next(&incoming), event("tide", json!({"high": true})));
        assert_eq!(answer(&link, Command::Ping), Ok("pong".into()));
        novad.stop();
        assert_eq!(next(&incoming), Incoming::Lost);
        assert_eq!(answer(&link, Command::Ping), Err(Refusal::NotConnected));
    }
}

#[test]
fn a_request_novad_never_answers_is_told_it_went_away() {
    use std::io::{BufRead, BufReader, Write};
    let dir = TempDir::new("client-unanswered");
    let listener = std::os::unix::net::UnixListener::bind(socket(&dir)).unwrap();
    let novad = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.write_all(b"{\"v\":2,\"event\":\"hello\",\"data\":{\"daemon\":\"silent\"}}\n").unwrap();
        let mut request = String::new();
        BufReader::new(stream.try_clone().unwrap()).read_line(&mut request).unwrap();
        request
    });
    let (link, incoming) = link(&socket(&dir));
    assert_eq!(next(&incoming), Incoming::Connected { daemon: "silent".into() });
    assert_eq!(answer(&link, Command::Ping), Err(Refusal::WentAway));
    assert_eq!(novad.join().unwrap().trim(), r#"{"v":2,"id":1,"cmd":"ping"}"#);
}

#[test]
fn another_protocol_version_is_no_connection() {
    use std::io::Write;
    let dir = TempDir::new("client-version");
    let listener = std::os::unix::net::UnixListener::bind(socket(&dir)).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let _ = stream.unwrap().write_all(b"{\"v\":99,\"event\":\"hello\",\"data\":{\"daemon\":\"new\"}}\n");
        }
    });
    let (_link, incoming) = link(&socket(&dir));
    assert!(incoming.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn recordings_are_read_from_a_folder() {
    let dir = TempDir::new("client-recorded");
    std::fs::write(dir.path().join("tide.jsonl"), "{\"event\":\"tide\",\"data\":{\"high\":false}}\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "not a recording").unwrap();
    let recorded = Recorded::new().read(dir.path()).unwrap();
    let _novad = FakeDaemon::start(&socket(&dir), recorded).unwrap();
    let (link, incoming) = link(&socket(&dir));
    link.subscribe(TIDE);
    assert_eq!(next(&incoming), connected());
    assert_eq!(next(&incoming), event("tide", json!({"high": false})));

    std::fs::write(dir.path().join("weather.jsonl"), "{\"data\":1}\n").unwrap();
    let problem = Recorded::new().read(dir.path()).unwrap_err();
    assert!(problem.contains("weather.jsonl: line 1: not an event"), "{problem}");
}

#[test]
fn asking_for_a_topic_again_sends_its_state_again() {
    let dir = TempDir::new("client-again");
    let _novad = FakeDaemon::start(&socket(&dir), recorded()).unwrap();
    let (link, incoming) = link(&socket(&dir));
    assert_eq!(next(&incoming), connected());
    for _ in 0..2 {
        link.subscribe(TIDE);
        assert_eq!(next(&incoming), event("tide", json!({"high": true})));
    }
}

#[test]
fn a_line_over_the_limit_is_an_error() {
    use crate::link::read_line;
    let mut line = Vec::new();
    let mut lines = "short\nfar too long\n".as_bytes();
    assert!(read_line(&mut lines, 8, &mut line).unwrap());
    assert_eq!(line, b"short\n");
    assert!(read_line(&mut lines, 8, &mut line).is_err());
    assert!(!read_line(&mut "".as_bytes(), 8, &mut line).unwrap());
}

#[test]
fn a_topic_nobody_asks_for_is_dropped_on_the_next_connection() {
    let dir = TempDir::new("client-unsubscribe");
    let (link, incoming) = link(&socket(&dir));
    link.subscribe(TIDE);
    link.subscribe(TIDE);
    link.subscribe(WEATHER);
    link.unsubscribe(WEATHER);
    link.unsubscribe(TIDE);
    let _novad = FakeDaemon::start(&socket(&dir), recorded()).unwrap();
    assert_eq!(next(&incoming), connected());
    assert_eq!(next(&incoming), event("tide", json!({"high": true})));
    assert!(incoming.recv_timeout(Duration::from_millis(100)).is_err());
}

#[test]
fn odd_lines_are_ignored() {
    use std::io::{BufRead, BufReader, Write};
    let dir = TempDir::new("client-odd");
    let listener = std::os::unix::net::UnixListener::bind(socket(&dir)).unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let lines = [
            r#"{"v":2,"event":"hello","data":{"daemon":"odd"}}"#,
            r#"{"v":2,"id":1,"result":"pong","event":"tide"}"#,
            r#"{"v":2,"id":1,"result":"pong","extra":true}"#,
            r#"{"v":2,"id":1}"#,
            r#"{"v":3,"id":1,"result":"pong"}"#,
            r#"{"v":2,"event":"tide","data":null,"more":1}"#,
            r#"{"v":2,"event":"tide"}"#,
            r#"{"v":2,"id":1,"result":null}"#,
        ];
        let mut request = String::new();
        stream.write_all(format!("{}\n", lines[0]).as_bytes()).unwrap();
        BufReader::new(stream.try_clone().unwrap()).read_line(&mut request).unwrap();
        stream.write_all(format!("{}\n", lines[1..].join("\n")).as_bytes()).unwrap();
        let _ = BufReader::new(stream).read_line(&mut request);
    });
    let (link, incoming) = link(&socket(&dir));
    assert_eq!(next(&incoming), Incoming::Connected { daemon: "odd".into() });
    assert_eq!(answer(&link, Command::Ping), Ok(Value::Null));
    assert_eq!(next(&incoming), event("tide", Value::Null));
    assert!(incoming.try_recv().is_err());
}
