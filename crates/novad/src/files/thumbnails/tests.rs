//! The thumbnail service through its real queues, cache and thumbnailer
//! runs, with a thumbnailer script that writes a real PNG.

use super::*;
use crate::hub::{ClientQueue, Outgoing, named_within, next_named};
use nova_protocol::{LogLevel, Topic};
use nova_test_support::{TempDir, write_program};
use serde_json::{Value, json};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::Receiver;

const WAIT: Duration = Duration::from_secs(10);

/// A home with photos, a MIME database that knows PNG, and a thumbnailer
/// for PNGs that copies a real one into place, counting its runs.
struct Fixture {
    dir: TempDir,
    hub: Arc<Hub>,
    thumbnails: Thumbnails,
    mime: Arc<Mime>,
}

impl Fixture {
    /// With the thumbnailer running `body` before it writes its picture
    /// (`$1` the input, `$2` the output).
    fn new(name: &str, body: &str) -> Self {
        let dir = TempDir::new(&format!("thumbnails-{name}"));
        let share = dir.join("prefix/share");
        std::fs::create_dir_all(share.join("mime")).unwrap();
        std::fs::write(share.join("mime/globs2"), "50:image/png:*.png\n50:text/plain:*.txt\n").unwrap();
        std::fs::create_dir_all(share.join("thumbnailers")).unwrap();
        std::fs::write(
            share.join("thumbnailers/png.thumbnailer"),
            "[Thumbnailer Entry]\nExec=png-thumbnailer %i %o\nMimeType=image/png;\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("prefix/bin")).unwrap();
        std::fs::write(dir.join("picture.png"), cache::tests::rgb_png(8, 8, [10, 200, 10])).unwrap();
        let picture = dir.join("picture.png").display().to_string();
        let runs = dir.join("runs").display().to_string();
        write_program(
            &dir.join("prefix/bin"),
            "png-thumbnailer",
            &format!("echo \"$1\" >> '{runs}'\n{body}\ncp '{picture}' \"$2\""),
        );
        std::fs::create_dir_all(dir.join("Pictures")).unwrap();
        let hub = Arc::new(Hub::default());
        let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
        let mime = Mime::follow(vec![share.clone()], Arc::clone(&logger));
        let sources = Sources {
            cache: Some(dir.join("cache/thumbnails")),
            data_dirs: vec![share],
            path_var: "/nonexistent".into(),
        };
        let thumbnails = Thumbnails::start(sources, Arc::clone(&hub), logger, Arc::clone(&mime));
        Self { dir, hub, thumbnails, mime }
    }

    fn client(&self) -> (ClientId, Receiver<Outgoing>, UnixStream) {
        let (stream, ours) = UnixStream::pair().unwrap();
        let (queue, outgoing) = ClientQueue::new(&ours).unwrap();
        let client = self.hub.register(queue);
        self.hub.subscribe(client, Topic::Files);
        (client, outgoing, stream)
    }

    /// A photo in Pictures, as a camera saves it.
    fn photo(&self, name: &str) -> String {
        let path = self.dir.join("Pictures").join(name);
        std::fs::write(&path, cache::tests::rgb_png(600, 400, [200, 10, 10])).unwrap();
        path.display().to_string()
    }

    /// How many times the thumbnailer ran.
    fn runs(&self) -> usize {
        std::fs::read_to_string(self.dir.join("runs")).map_or(0, |runs| runs.lines().count())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.thumbnails.stop();
        self.mime.stop();
    }
}

fn answer(events: &Receiver<Outgoing>) -> Value {
    next_named(events, "files_thumbnail", WAIT)
}

/// When the file at `path` last changed, as a listing shows it.
fn modified_ms(path: &str) -> Value {
    json!(crate::files::folder::modified_ms(&std::fs::metadata(path).unwrap()))
}

#[test]
fn a_photo_gets_a_thumbnail_made_once_and_then_found_in_the_cache() {
    let fixture = Fixture::new("made", "");
    let (client, events, _stream) = fixture.client();
    let photo = fixture.photo("beach.png");
    fixture.thumbnails.request(client, &photo).unwrap();
    let made = answer(&events);
    assert_eq!(made["path"], json!(photo));
    assert_eq!(made["modified_ms"], modified_ms(&photo), "it says which version of the file it shows");
    let thumbnail = made["thumbnail"].as_str().expect("a thumbnail").to_string();
    assert!(thumbnail.starts_with(&fixture.dir.join("cache/thumbnails/large/").display().to_string()));
    assert!(Path::new(&thumbnail).is_file());

    fixture.thumbnails.request(client, &photo).unwrap();
    assert_eq!(answer(&events), json!({"path": photo, "modified_ms": modified_ms(&photo), "thumbnail": thumbnail}));
    assert_eq!(fixture.runs(), 1, "the second came from the cache");
    let leftovers = std::fs::read_dir(fixture.dir.join("cache/thumbnails/large")).unwrap().count();
    assert_eq!(leftovers, 1, "the thumbnailer's own file is cleaned up");
}

#[test]
fn a_file_no_thumbnailer_takes_has_none() {
    let fixture = Fixture::new("none", "");
    let (client, events, _stream) = fixture.client();
    let notes = fixture.dir.join("Pictures/notes.txt");
    std::fs::write(&notes, "shopping").unwrap();
    let notes = notes.display().to_string();
    fixture.thumbnails.request(client, &notes).unwrap();
    assert_eq!(answer(&events), json!({"path": notes, "modified_ms": modified_ms(&notes)}));
    let missing = fixture.dir.join("Pictures/gone.png").display().to_string();
    fixture.thumbnails.request(client, &missing).unwrap();
    assert_eq!(answer(&events), json!({"path": missing}));
    assert!(fixture.thumbnails.request(client, "relative.png").is_err());
    assert_eq!(fixture.runs(), 0);
}

#[test]
fn a_file_the_thumbnailer_fails_on_is_not_tried_again_until_it_changes() {
    let fixture = Fixture::new("fails", "exit 3");
    let (client, events, _stream) = fixture.client();
    let photo = fixture.photo("broken.png");
    fixture.thumbnails.request(client, &photo).unwrap();
    assert_eq!(answer(&events), json!({"path": photo, "modified_ms": modified_ms(&photo)}));
    fixture.thumbnails.request(client, &photo).unwrap();
    assert_eq!(answer(&events), json!({"path": photo, "modified_ms": modified_ms(&photo)}));
    assert_eq!(fixture.runs(), 1, "the failure was remembered");

    let file = std::fs::File::options().write(true).open(&photo).unwrap();
    file.set_modified(std::time::SystemTime::now() + Duration::from_secs(120)).unwrap();
    fixture.thumbnails.request(client, &photo).unwrap();
    assert_eq!(answer(&events), json!({"path": photo, "modified_ms": modified_ms(&photo)}));
    assert_eq!(fixture.runs(), 2, "a changed file is tried again");
}

#[test]
fn what_scrolled_away_is_not_made_and_the_latest_goes_first() {
    // Each run waits until the test lets it go, all of them (`go`) or its
    // own file (`go-<name>`), so both makers are busy while more are asked
    // for, and then one maker at a time takes from the queue.
    let fixture = Fixture::new(
        "cancel",
        "cache=\"$(dirname \"$2\")/../..\"\n\
         while [ ! -e \"$cache/go\" ] && [ ! -e \"$cache/go-$(basename \"$1\")\" ]; do sleep 0.02; done",
    );
    let (client, events, _stream) = fixture.client();
    let photos: Vec<String> = (0..5).map(|n| fixture.photo(&format!("{n}.png"))).collect();
    for photo in &photos[..2] {
        fixture.thumbnails.request(client, photo).unwrap();
    }
    nova_test_support::wait_until(WAIT, "both makers busy", || fixture.runs() == 2);
    for photo in &photos[2..] {
        fixture.thumbnails.request(client, photo).unwrap();
    }
    fixture.thumbnails.cancel(client, &photos[3]);
    nova_test_support::wait_until(WAIT, "both looked up", || {
        lock(&fixture.thumbnails.shared.queues).to_make.len() == 2
    });
    // One maker comes free and takes the latest asked for; the other is
    // still busy, so nothing races it.
    std::fs::write(fixture.dir.join("cache/go-0.png"), "").unwrap();
    nova_test_support::wait_until(WAIT, "a third run", || fixture.runs() == 3);
    std::fs::write(fixture.dir.join("cache/go"), "").unwrap();
    let mut answered: Vec<String> = (0..4).map(|_| answer(&events)["path"].as_str().unwrap().to_string()).collect();
    answered.sort();
    assert_eq!(answered, [&photos[0], &photos[1], &photos[2], &photos[4]].map(String::clone));
    assert!(
        named_within(&events, "files_thumbnail", Duration::from_millis(300)).is_none(),
        "the cancelled one never comes"
    );
    let runs = std::fs::read_to_string(fixture.dir.join("runs")).unwrap();
    let started: Vec<&str> = runs.lines().skip(2).collect();
    assert_eq!(started, [photos[4].as_str(), photos[2].as_str()], "the latest asked for starts first");
}

#[test]
fn a_client_waiting_for_too_many_has_its_oldest_answered_with_none() {
    // Blocked, so nothing is made while the queue fills.
    let fixture = Fixture::new("limit", "while [ ! -e \"$(dirname \"$2\")/../../go\" ]; do sleep 0.02; done");
    let (client, events, _stream) = fixture.client();
    let busy: Vec<String> = (0..MAKERS).map(|n| fixture.photo(&format!("busy-{n}.png"))).collect();
    for photo in &busy {
        fixture.thumbnails.request(client, photo).unwrap();
    }
    nova_test_support::wait_until(WAIT, "the makers busy", || fixture.runs() == MAKERS);
    let many = |n: usize| fixture.dir.join(format!("Pictures/many-{n}.png")).display().to_string();
    for n in 0..=FILES_THUMBNAILS_MAX_WAITING {
        std::fs::copy(&busy[0], many(n)).unwrap();
    }
    for n in 0..=FILES_THUMBNAILS_MAX_WAITING {
        fixture.thumbnails.request(client, &many(n)).unwrap();
    }
    assert_eq!(answer(&events), json!({"path": many(0)}), "the longest waiting goes");
}

#[test]
fn a_thumbnailer_that_cant_start_leaves_the_file_to_try_again() {
    let fixture = Fixture::new("cant-start", "");
    let (client, events, _stream) = fixture.client();
    let program = fixture.dir.join("prefix/bin/png-thumbnailer");
    let kept = std::fs::read(&program).unwrap();
    let photo = fixture.photo("later.png");
    // Read as the first ask comes, then gone, as when a package is removed.
    let notes = fixture.dir.join("Pictures/first.txt");
    std::fs::write(&notes, "x").unwrap();
    fixture.thumbnails.request(client, &notes.display().to_string()).unwrap();
    answer(&events);
    std::fs::remove_file(&program).unwrap();
    fixture.thumbnails.request(client, &photo).unwrap();
    assert_eq!(answer(&events)["thumbnail"], Value::Null);

    // Back, executable, with the script it had.
    write_program(&fixture.dir.join("prefix/bin"), "png-thumbnailer", "");
    std::fs::write(&program, kept).unwrap();
    fixture.thumbnails.request(client, &photo).unwrap();
    assert!(answer(&events)["thumbnail"].is_string(), "not marked failed, so it's made now");
}

#[test]
fn a_file_changed_while_its_thumbnail_is_made_is_made_again() {
    let fixture = Fixture::new("changed", "while [ ! -e \"$(dirname \"$2\")/../../go\" ]; do sleep 0.02; done");
    let (client, events, _stream) = fixture.client();
    let photo = fixture.photo("edited.png");
    fixture.thumbnails.request(client, &photo).unwrap();
    nova_test_support::wait_until(WAIT, "it's being made", || fixture.runs() == 1);
    let file = std::fs::File::options().write(true).open(&photo).unwrap();
    file.set_modified(std::time::SystemTime::now() + Duration::from_secs(120)).unwrap();
    fixture.thumbnails.request(client, &photo).unwrap();
    nova_test_support::wait_until(WAIT, "looked up", || lock(&fixture.thumbnails.shared.queues).to_make.len() == 1);
    std::fs::write(fixture.dir.join("cache/go"), "").unwrap();
    let made = answer(&events);
    assert_eq!(made["modified_ms"], modified_ms(&photo), "only the edited version's answer comes");
    assert!(made["thumbnail"].is_string());
    assert!(named_within(&events, "files_thumbnail", Duration::from_millis(300)).is_none());
    assert_eq!(fixture.runs(), 2);
}

#[test]
fn a_thumbnailer_installed_later_is_used_without_a_restart() {
    let fixture = Fixture::new("installed", "");
    let entry = fixture.dir.join("prefix/share/thumbnailers/png.thumbnailer");
    let text = std::fs::read_to_string(&entry).unwrap();
    std::fs::remove_file(&entry).unwrap();
    let (client, events, _stream) = fixture.client();
    let before = fixture.photo("before.png");
    fixture.thumbnails.request(client, &before).unwrap();
    assert_eq!(answer(&events)["thumbnail"], Value::Null, "no thumbnailer yet");

    std::fs::write(&entry, text).unwrap();
    let after = fixture.photo("after.png");
    fixture.thumbnails.request(client, &after).unwrap();
    assert!(answer(&events)["thumbnail"].is_string(), "the new thumbnailer was read");
}
