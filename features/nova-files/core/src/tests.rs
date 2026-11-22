//! The client, and the launcher's Files mode, against the stand-in for Nova
//! Files (`stand_in`).

use super::stand_in::{StandIn, client, nova_files};
use super::{NovaFiles, STAGES_WAIT};
use nova_base::lock;
use nova_session_bus::TestBus;
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
    let dir = TempDir::new("nova-files-unreachable");
    let unreachable =
        NovaFiles { client: Err("no bus".into()), mime: super::stand_in::mime(&dir), home: None, own_windows: true };
    assert_eq!(unreachable.search("kit", 30).unwrap_err(), "Nova Files isn't reachable: no bus");
}

/// Runs `id` as the launcher does, Shift+Enter when `other_way`.
fn run(files: &NovaFiles, id: &str, other_way: bool) -> Result<(), String> {
    files.run_item(id, other_way)
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
    let files = nova_files(&bus.address, &dir, Some(home.clone()), false);

    let found = files.items("kit").unwrap();
    let names: Vec<String> = found.iter().map(|item| item.name.iter().map(|run| run.text.as_str()).collect()).collect();
    assert_eq!(names, ["kitchen.txt", "kitcen notes.md"]);
    let folder = &found[0];
    assert_eq!(folder.id, root.join("kitchen.txt").display().to_string());
    assert_eq!(folder.name[0], nova_launcher_api::TextRun { text: "kit".into(), matched: true });
    assert_eq!(folder.description.as_deref(), Some("~/.notes"));
    let file = folder.file.as_ref().unwrap();
    assert!(file.dir && file.hidden, "a folder, inside a dot folder");
    assert_eq!(file.mime, nova_mime::DIRECTORY);
    assert_eq!(file.completion, "~/.notes/kitchen.txt/");
    assert!(!found[1].file.as_ref().unwrap().dir);

    // Gone since it was found: left out.
    std::fs::remove_file(root.join("kitcen notes.md")).unwrap();
    assert_eq!(files.items("kit").unwrap().len(), 1);

    run(&files, &found[1].id, false).unwrap();
    run(&files, &found[1].id, true).unwrap();
    assert!(run(&files, "relative/path", false).is_err(), "only paths Nova Files found run");
    assert!(run(&files, "/home/me/../../etc/shadow", false).is_err());
    let asked = lock(&stand_in.asked);
    assert_eq!(asked.opened, [found[1].id.clone()]);
    assert_eq!(asked.shown, [found[1].id.clone()]);
    assert!(asked.folders_elsewhere.is_empty() && asked.items_elsewhere.is_empty());
}

/// In the launcher mode Nova Files has no windows to show: folders open,
/// and Shift+Enter shows items, in the user's file manager; files still
/// open in their default app through Nova Files.
#[test]
fn in_the_launcher_mode_folders_open_in_the_users_file_manager() {
    let dir = TempDir::new("nova-files-launcher-mode");
    let root = dir.join("my files");
    std::fs::create_dir_all(root.join("kitchen.txt")).unwrap();
    std::fs::write(root.join("kitcen notes.md"), b"x").unwrap();
    let bus = TestBus::start(&dir.join("bus"));
    let stand_in = StandIn::serve(&bus.address, &root.display().to_string());
    let files = nova_files(&bus.address, &dir, None, true);
    let found = files.items("kit").unwrap();
    let (folder, file) = (&found[0].id, &found[1].id);

    run(&files, folder, false).unwrap();
    run(&files, file, false).unwrap();
    run(&files, file, true).unwrap();
    let asked = lock(&stand_in.asked);
    let uri = |path: &str| super::client::file_uri(std::path::Path::new(path));
    assert_eq!(asked.folders_elsewhere, [uri(folder)]);
    assert!(asked.folders_elsewhere[0].ends_with("/my%20files/kitchen.txt"), "{:?}", asked.folders_elsewhere);
    assert_eq!(asked.opened, *std::slice::from_ref(file));
    assert_eq!(asked.items_elsewhere, [uri(file)]);
    assert!(asked.shown.is_empty(), "no Nova Files window");
}

#[test]
fn a_search_nova_files_cannot_run_answers_with_why() {
    let fixture = Fixture::new("launcher-failed");
    let files = nova_files(&fixture.bus.address, &fixture._dir, None, false);
    let found = files.items("broken").unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].name[0].text.contains("index-failed"));
    assert!(found[0].id.is_empty() && found[0].file.is_none(), "nothing to run");
}

/// A typed path is completed by Nova Files from its folder, not searched
/// for, and each item's completion keeps the spelling that was typed.
#[test]
fn a_typed_path_completes_through_nova_files_in_its_own_spelling() {
    let dir = TempDir::new("nova-files-typed");
    let home = dir.join("home");
    let root = home.join(".notes");
    std::fs::create_dir_all(root.join("kitchen.txt")).unwrap();
    std::fs::write(root.join("kitcen notes.md"), b"x").unwrap();
    let bus = TestBus::start(&dir.join("bus"));
    let stand_in = StandIn::serve(&bus.address, &root.display().to_string());
    let files = nova_files(&bus.address, &dir, Some(home.clone()), false);

    let found = files.items("~/.notes/Kit").unwrap();
    let completions: Vec<&str> = found.iter().map(|item| item.file.as_ref().unwrap().completion.as_str()).collect();
    assert_eq!(completions, ["~/.notes/kitchen.txt/", "~/.notes/kitcen notes.md"]);
    assert_eq!(found[0].id, root.join("kitchen.txt").display().to_string());
    assert_eq!(found[0].name[0], nova_launcher_api::TextRun { text: "kit".into(), matched: true });
    assert_eq!(found[0].description.as_deref(), Some("~/.notes"));

    let typed = format!("{}/kit", root.display());
    let found = files.items(&typed).unwrap();
    assert_eq!(found[0].file.as_ref().unwrap().completion, format!("{}/kitchen.txt/", root.display()));

    let failed = files.items("~nobody/kit").unwrap();
    assert!(failed[0].id.is_empty() && failed[0].file.is_none(), "one item saying why");
    assert!(files.items("~/Nowhere").unwrap().is_empty());

    let asked = lock(&stand_in.asked);
    assert!(asked.searches.is_empty(), "typed paths aren't name searches");
    assert_eq!(asked.completions[0].0, "~/.notes/Kit");
}
