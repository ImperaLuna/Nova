//! The client against the stand-in for Nova Files (`stand_in`).

use super::stand_in::{StandIn, client};
use super::{NovaFiles, STAGES_WAIT};
use crate::lock;
use crate::session_bus::TestBus;
use nova_test_support::TempDir;
use std::path::PathBuf;
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
    let unreachable = NovaFiles { client: Err("no bus".into()) };
    assert_eq!(unreachable.search("kit", 30).unwrap_err(), "Nova Files isn't reachable: no bus");
}
