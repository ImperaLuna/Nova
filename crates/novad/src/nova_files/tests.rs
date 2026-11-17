//! The client, and the launcher's Files mode, against the stand-in for Nova
//! Files (`stand_in`).

use super::stand_in::{StandIn, client, nova_files};
use super::{NovaFiles, STAGES_WAIT};
use crate::launcher::providers::{Provider, Run};
use crate::lock;
use crate::logger::Logger;
use crate::session_bus::TestBus;
use nova_protocol::LogLevel;
use nova_test_support::TempDir;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(5);
const ROOT: &str = "/h";

/// A private bus with the stand-in serving on it as Nova Files.
struct Fixture {
    _dir: TempDir,
    bus: TestBus,
    stand_in: StandIn,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = TempDir::new(&format!("nova-files-{name}"));
        let bus = TestBus::start(&dir.join("bus"));
        let stand_in = StandIn::serve(&bus.address, ROOT);
        Self { _dir: dir, bus, stand_in }
    }

    fn client(&self) -> super::client::Client {
        client(&self.bus.address)
    }
}

fn paths(found: &[&str]) -> Vec<PathBuf> {
    found.iter().map(PathBuf::from).collect()
}

#[test]
fn a_search_gathers_every_stage_until_done() {
    let fixture = Fixture::new("stages");
    let client = fixture.client();
    let found = client.search("kit", 30, TIMEOUT).unwrap();
    assert_eq!(found, paths(&["/h/kitchen.txt", "/h/kitcen notes.md"]));
    assert_eq!(client.search("nothing", 30, TIMEOUT).unwrap(), Vec::<PathBuf>::new());
    let asked = lock(&fixture.stand_in.asked);
    assert_eq!(asked.searches, [(1, "kit".to_string(), 30), (2, "nothing".to_string(), 30)]);
    assert_eq!(asked.cancelled, Vec::<u64>::new(), "finished searches need no cancel");
}

#[test]
fn past_the_wait_the_stages_so_far_answer_and_the_next_search_cancels_it() {
    let fixture = Fixture::new("slow");
    let client = fixture.client();
    let started = Instant::now();
    assert_eq!(client.search("slow", 30, STAGES_WAIT).unwrap(), paths(&["/h/slow.txt"]));
    assert!(started.elapsed() < TIMEOUT, "answered after the wait, not the call timeout");
    client.search("kit", 30, TIMEOUT).unwrap();
    // The cancel is sent without a reply; the search after it comes later.
    client.search("nothing", 30, TIMEOUT).unwrap();
    assert_eq!(lock(&fixture.stand_in.asked).cancelled, [1]);
}

#[test]
fn a_failed_search_says_why() {
    let fixture = Fixture::new("failed");
    let error = fixture.client().search("broken", 30, TIMEOUT).unwrap_err();
    assert!(error.contains("index-failed"), "{error}");
}

#[test]
fn without_nova_files_on_the_bus_a_search_fails() {
    let dir = TempDir::new("nova-files-absent");
    let bus = TestBus::start(&dir.join("bus"));
    let client = client(&bus.address);
    assert!(client.search("kit", 30, TIMEOUT).is_err());
}

#[test]
fn a_feature_that_cannot_connect_says_so() {
    let dir = TempDir::new("nova-files-unreachable");
    let unreachable = NovaFiles { client: Err("no bus".into()), mime: super::stand_in::mime(&dir), home: None };
    assert_eq!(unreachable.search("kit", 30).unwrap_err(), "Nova Files isn't reachable: no bus");
}

/// Runs `id` as the launcher does, Shift+Enter when `other_way`.
fn run(provider: &dyn Provider, dir: &TempDir, id: &str, other_way: bool) -> Result<(), String> {
    let logger = Logger::open(&dir.join("run-logs"), LogLevel::Debug, false, Arc::default()).unwrap();
    let how =
        Run { output: None, new_window: other_way, compositor: None, tray: None, logger: &logger, underway: &|| {} };
    provider.run(id, &how)
}

#[test]
fn the_launchers_files_mode_finds_opens_and_shows_files_through_nova_files() {
    let dir = TempDir::new("nova-files-launcher");
    let home = dir.join("home");
    // What the stand-in finds for `kit`, under a dot folder in Home: a
    // folder named like a file, and a file.
    let root = home.join(".notes");
    std::fs::create_dir_all(root.join("kitchen.txt")).unwrap();
    std::fs::write(root.join("kitcen notes.md"), b"x").unwrap();
    let bus = TestBus::start(&dir.join("bus"));
    let stand_in = StandIn::serve(&bus.address, &root.display().to_string());
    let files = nova_files(&bus.address, &dir, Some(home.clone()));
    assert!(files.own_mode());

    let found = Provider::search(&files, "kit").unwrap();
    let names: Vec<String> = found.iter().map(|item| item.name.iter().map(|run| run.text.as_str()).collect()).collect();
    assert_eq!(names, ["kitchen.txt", "kitcen notes.md"]);
    let folder = &found[0];
    assert_eq!(folder.id, root.join("kitchen.txt").display().to_string());
    assert_eq!(folder.name[0], nova_protocol::TextRun { text: "kit".into(), matched: true });
    assert_eq!(folder.description.as_deref(), Some("~/.notes"));
    let file = folder.file.as_ref().unwrap();
    assert!(file.dir && file.hidden, "a folder, inside a dot folder");
    assert_eq!(file.mime, crate::mime::DIRECTORY);
    assert_eq!(file.completion, "~/.notes/kitchen.txt/");
    assert!(!found[1].file.as_ref().unwrap().dir);

    // Gone since it was found: left out.
    std::fs::remove_file(root.join("kitcen notes.md")).unwrap();
    assert_eq!(Provider::search(&files, "kit").unwrap().len(), 1);

    run(&files, &dir, &found[1].id, false).unwrap();
    run(&files, &dir, &found[1].id, true).unwrap();
    assert!(run(&files, &dir, "relative/path", false).is_err(), "only paths Nova Files found run");
    assert!(run(&files, &dir, "/home/me/../../etc/shadow", false).is_err());
    let asked = lock(&stand_in.asked);
    assert_eq!(asked.opened, [found[1].id.clone()]);
    assert_eq!(asked.shown, [found[1].id.clone()]);
}

#[test]
fn a_search_nova_files_cannot_run_answers_with_why() {
    let fixture = Fixture::new("launcher-failed");
    let files = nova_files(&fixture.bus.address, &fixture._dir, None);
    let found = Provider::search(&files, "broken").unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].name[0].text.contains("index-failed"));
    assert!(found[0].id.is_empty() && found[0].file.is_none(), "nothing to run");
}
