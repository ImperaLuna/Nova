//! Undo and redo (`files_undo`, `files_redo`, Ctrl+Z and Ctrl+Y): one
//! history for the session, shared by every Nova Files window, so a step
//! taken in one window can be undone from another.
//!
//! Each step is kept as what undoing it does ([`Op`]): copies go to the
//! trash, trashed items are restored, moved items move back. Running an
//! op answers its inverse, which goes on the other stack, so undo and redo
//! are the same code. What an op couldn't do stays on its own stack, to be
//! tried again. Trashing and restoring run before the request is answered;
//! a move back is a `files_transfer` job of the asking connection, with its
//! progress card and name clashes, and the job tells the history what it
//! moved when it ends ([`History::moved`]).
//!
//! Steps not taken in Nova Files (another app renaming, a file deleted)
//! aren't known: an op then fails on the item it can't find, as Explorer's
//! does. Deleting for good is no step: it can't be undone.
//!
//! The stacks and the [`Event::FilesHistory`] last published change under
//! one lock, so windows never see an older state after a newer one.

use crate::files::Trash;
use crate::files::transfers::{Leg, Transfers};
use crate::hub::{ClientId, Hub};
use crate::lock;
use nova_protocol::{Event, FileStep, TransferMode, wire_text};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Steps kept on each stack; the oldest go past it.
const MOST_STEPS: usize = 100;

/// What undoing (or redoing) a step does.
#[derive(Debug, Clone, PartialEq)]
enum Op {
    /// Moves the items at these paths (wire text) to the trash.
    Trash(Vec<String>),
    /// Restores these trashed items (trash ids).
    Restore(Vec<String>),
    /// Moves each item from where it is to where it goes: (now, goes).
    Move(Vec<(PathBuf, PathBuf)>),
}

impl Op {
    fn is_empty(&self) -> bool {
        match self {
            Op::Trash(items) | Op::Restore(items) => items.is_empty(),
            Op::Move(items) => items.is_empty(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Step {
    kind: FileStep,
    op: Op,
}

/// Which way a step runs: undoing takes it from the undo stack and leaves
/// its inverse on the redo stack, redoing the other way round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    Undo,
    Redo,
}

/// Where a copy or move job came from, for what it records when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A copy or move the user started: a new step.
    New,
    /// Undoing or redoing a move.
    Step(Way),
}

#[derive(Default)]
struct Stacks {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// What was last published: the kinds on top of each stack.
    shown: (Option<FileStep>, Option<FileStep>),
}

impl Stacks {
    fn pushed(stack: &mut Vec<Step>, step: Step) {
        if step.op.is_empty() {
            return;
        }
        stack.push(step);
        if stack.len() > MOST_STEPS {
            stack.remove(0);
        }
    }

    fn from(&mut self, way: Way) -> &mut Vec<Step> {
        match way {
            Way::Undo => &mut self.undo,
            Way::Redo => &mut self.redo,
        }
    }

    fn to(&mut self, way: Way) -> &mut Vec<Step> {
        match way {
            Way::Undo => &mut self.redo,
            Way::Redo => &mut self.undo,
        }
    }

    /// A step the user just took: it can be undone, and what was undone
    /// can no longer be redone.
    fn took(&mut self, step: Step) {
        if step.op.is_empty() {
            return;
        }
        self.redo.clear();
        Self::pushed(&mut self.undo, step);
    }

    /// A step ran `way`: `done` is the inverse of what it did, `left` what
    /// it couldn't do.
    fn ran(&mut self, way: Way, kind: FileStep, done: Op, left: Op) {
        Self::pushed(self.from(way), Step { kind, op: left });
        Self::pushed(self.to(way), Step { kind, op: done });
    }
}

pub struct History {
    hub: Arc<Hub>,
    stacks: Mutex<Stacks>,
}

impl History {
    pub fn new(hub: Arc<Hub>) -> Self {
        Self { hub, stacks: Mutex::default() }
    }

    /// Sends client `client`, which just subscribed, what can be undone and
    /// redone.
    pub fn send_state(&self, client: ClientId) {
        let (undo, redo) = lock(&self.stacks).shown;
        self.hub.send_to(client, &Event::FilesHistory { undo, redo });
    }

    /// Items were moved to the trash as `ids`: one step, or part of the
    /// last one when it `continues` a trashing too big for one request.
    pub fn trashed(&self, ids: Vec<String>, continues: bool) {
        self.change(|stacks| {
            if continues && let Some(Step { kind: FileStep::Trash, op: Op::Restore(last) }) = stacks.undo.last_mut() {
                last.extend(ids);
                return;
            }
            stacks.took(Step { kind: FileStep::Trash, op: Op::Restore(ids) });
        });
    }

    /// A copy or move job from `origin` ended, having placed each item
    /// (from, to): a new step, or the inverse of a step run. `left` is what
    /// a step's move didn't move, back on its stack.
    pub fn moved(
        &self,
        origin: Origin,
        mode: TransferMode,
        placed: &[(PathBuf, PathBuf)],
        left: Vec<(PathBuf, PathBuf)>,
    ) {
        let back = || Op::Move(placed.iter().map(|(from, to)| (to.clone(), from.clone())).collect());
        self.change(|stacks| match (origin, mode) {
            (Origin::New, TransferMode::Copy) => {
                let copies = placed.iter().map(|(_, to)| wire_text(to.as_os_str())).collect();
                stacks.took(Step { kind: FileStep::Copy, op: Op::Trash(copies) });
            }
            (Origin::New, TransferMode::Move) => stacks.took(Step { kind: FileStep::Move, op: back() }),
            (Origin::Step(way), _) => stacks.ran(way, FileStep::Move, back(), Op::Move(left)),
        });
    }

    /// Undoes or redoes the last step, `way`. Answers whether that started
    /// client `client`'s move job `job`; anything else is done by then.
    /// The error names what couldn't be done, which stays on its stack.
    pub fn run(
        &self,
        way: Way,
        trash: &Trash,
        transfers: &Transfers,
        client: ClientId,
        job: u32,
    ) -> Result<bool, String> {
        let Some(Step { kind, op }) = self.change(|stacks| stacks.from(way).pop()) else { return Ok(false) };
        match op {
            Op::Trash(paths) => {
                let outcomes = trash.trash_each(&paths);
                let (done, left, problems) = split(&paths, outcomes);
                self.change(|stacks| stacks.ran(way, kind, Op::Restore(done), Op::Trash(left)));
                problems.map(|()| false)
            }
            Op::Restore(ids) => {
                let outcomes = trash.restore_each(&ids);
                let (done, left, problems) = split(&ids, outcomes);
                let done = done.iter().map(|path| wire_text(path.as_os_str())).collect();
                self.change(|stacks| stacks.ran(way, kind, Op::Trash(done), Op::Restore(left)));
                problems.map(|()| false)
            }
            Op::Move(items) => {
                let started = transfers.start_legs(client, job, TransferMode::Move, legs(&items)?, Origin::Step(way));
                if let Err(e) = started {
                    self.change(|stacks| Stacks::pushed(stacks.from(way), Step { kind, op: Op::Move(items) }));
                    return Err(e);
                }
                Ok(true)
            }
        }
    }

    /// Changes the stacks, publishing what can be undone and redone when
    /// that changed.
    fn change<T>(&self, change: impl FnOnce(&mut Stacks) -> T) -> T {
        let mut stacks = lock(&self.stacks);
        let changed = change(&mut stacks);
        let shown = (stacks.undo.last().map(|step| step.kind), stacks.redo.last().map(|step| step.kind));
        if shown != stacks.shown {
            stacks.shown = shown;
            self.hub.publish(&Event::FilesHistory { undo: shown.0, redo: shown.1 });
        }
        changed
    }
}

/// `items` split by `outcomes`, one each: what each that worked gave, the
/// items that didn't, and why not.
fn split<T>(items: &[String], outcomes: Vec<Result<T, String>>) -> (Vec<T>, Vec<String>, Result<(), String>) {
    let mut done = Vec::new();
    let mut left = Vec::new();
    let mut problems = Vec::new();
    for (item, outcome) in items.iter().zip(outcomes) {
        match outcome {
            Ok(value) => done.push(value),
            Err(e) => {
                left.push(item.clone());
                problems.push(e);
            }
        }
    }
    (done, left, if problems.is_empty() { Ok(()) } else { Err(problems.join("\n")) })
}

/// A move's items as one leg per folder they go to, each made again if
/// it's gone (a folder merged away when they first moved).
fn legs(items: &[(PathBuf, PathBuf)]) -> Result<Vec<Leg>, String> {
    let mut folders: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for (now, goes) in items {
        let folder = goes.parent().ok_or_else(|| format!("Can't move {} to /", now.display()))?;
        folders.entry(folder.to_owned()).or_default().push(now.clone());
    }
    folders
        .into_iter()
        .map(|(folder, sources)| {
            std::fs::create_dir_all(&folder).map_err(|e| format!("Can't make {} again: {e}", folder.display()))?;
            Ok(Leg { sources, folder })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{ClientQueue, Outgoing, next_event};
    use crate::logger::Logger;
    use crate::mime::Mime;
    use nova_protocol::{LogLevel, Transfer, TransferState};
    use nova_test_support::TempDir;
    use serde_json::json;
    use std::cell::{Cell, RefCell};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(5);

    /// A home trash, copy and move jobs and the history, with a `from` and
    /// a `to` folder, and one client hearing the files topic.
    struct Fixture {
        dir: TempDir,
        history: Arc<History>,
        trash: Trash,
        transfers: Transfers,
        client: ClientId,
        events: Receiver<Outgoing>,
        _socket: UnixStream,
        next_job: Cell<u32>,
        /// The latest `files_history` heard.
        history_shown: RefCell<Option<serde_json::Value>>,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir = TempDir::new(&format!("files-history-{name}"));
            for folder in ["from", "to"] {
                std::fs::create_dir_all(dir.join(folder)).unwrap();
            }
            std::fs::write(dir.join("mountinfo"), "").unwrap();
            let hub = Arc::new(Hub::default());
            let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
            let mime = Mime::follow(Vec::new(), Arc::clone(&logger));
            let trash = Trash::new(Some(dir.join("Trash")), dir.join("mountinfo"), mime);
            let history = Arc::new(History::new(Arc::clone(&hub)));
            let transfers = Transfers::new(Arc::clone(&hub), logger, Arc::clone(&history));
            let (socket, ours) = UnixStream::pair().unwrap();
            let (queue, events) = ClientQueue::new(&ours).unwrap();
            let client = hub.register(queue);
            hub.subscribe(client, nova_protocol::Topic::Files);
            Self {
                dir,
                history,
                trash,
                transfers,
                client,
                events,
                _socket: socket,
                next_job: Cell::new(1),
                history_shown: RefCell::default(),
            }
        }

        fn write(&self, path: &str) {
            let path = self.dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "hi").unwrap();
        }

        fn exists(&self, path: &str) -> bool {
            self.dir.join(path).symlink_metadata().is_ok()
        }

        fn text(&self, path: &str) -> String {
            self.dir.join(path).display().to_string()
        }

        fn job(&self) -> u32 {
            let job = self.next_job.get();
            self.next_job.set(job + 1);
            job
        }

        /// Copies or moves `sources` into `folder` and waits for it to end.
        fn transfer(&self, mode: TransferMode, sources: &[&str], folder: &str) {
            let sources: Vec<_> = sources.iter().map(|source| self.text(source)).collect();
            let job = self.job();
            self.transfers.start(self.client, job, mode, &sources, &self.text(folder)).unwrap();
            self.ended(TransferState::Done);
        }

        /// Undoes or redoes the last step, waiting for a move it starts.
        fn run(&self, way: Way) -> Result<bool, String> {
            let started = self.history.run(way, &self.trash, &self.transfers, self.client, self.job());
            if started == Ok(true) {
                self.ended(TransferState::Done);
            }
            started
        }

        /// The next job report, keeping the history heard on the way.
        fn next_transfer(&self) -> Transfer {
            loop {
                let event = next_event(&self.events, WAIT);
                match event["event"].as_str() {
                    Some("files_transfer") => return serde_json::from_value(event["data"].clone()).unwrap(),
                    Some("files_history") => *self.history_shown.borrow_mut() = Some(event["data"].clone()),
                    _ => {}
                }
            }
        }

        fn ended(&self, expected: TransferState) {
            loop {
                let transfer = self.next_transfer();
                if matches!(transfer.state, TransferState::Done | TransferState::Cancelled | TransferState::Failed) {
                    assert_eq!(transfer.state, expected, "{:?}", transfer.error);
                    return;
                }
            }
        }

        /// The latest `files_history` published.
        fn shown(&self) -> serde_json::Value {
            while let Ok(Outgoing::Line(line)) = self.events.try_recv() {
                let message: serde_json::Value = serde_json::from_str(&line).unwrap();
                if message["event"] == "files_history" {
                    *self.history_shown.borrow_mut() = Some(message["data"].clone());
                }
            }
            self.history_shown.borrow().clone().expect("a files_history event")
        }

        fn trashed(&self) -> usize {
            self.trash.list().len()
        }
    }

    #[test]
    fn a_copy_is_undone_to_the_trash_and_redone_from_it() {
        let fixture = Fixture::new("copy");
        fixture.write("from/notes.txt");
        fixture.transfer(TransferMode::Copy, &["from/notes.txt"], "to");
        assert_eq!(fixture.shown(), json!({"undo": "copy"}));
        assert_eq!(fixture.run(Way::Undo), Ok(false));
        assert!(!fixture.exists("to/notes.txt") && fixture.exists("from/notes.txt"));
        assert_eq!(fixture.trashed(), 1);
        assert_eq!(fixture.shown(), json!({"redo": "copy"}));
        assert_eq!(fixture.run(Way::Redo), Ok(false));
        assert!(fixture.exists("to/notes.txt"));
        assert_eq!(fixture.trashed(), 0);
        assert_eq!(fixture.shown(), json!({"undo": "copy"}));
    }

    #[test]
    fn a_move_goes_back_even_out_of_a_merged_folder() {
        let fixture = Fixture::new("move");
        fixture.write("from/Photos/a.png");
        fixture.write("from/notes.txt");
        fixture.write("to/Photos/b.png");
        let sources: Vec<_> = ["from/Photos", "from/notes.txt"].iter().map(|source| fixture.text(source)).collect();
        fixture
            .transfers
            .start(fixture.client, fixture.job(), TransferMode::Move, &sources, &fixture.text("to"))
            .unwrap();
        let asking = loop {
            let transfer = fixture.next_transfer();
            if transfer.state == TransferState::Asking {
                break transfer;
            }
        };
        let clash = asking.clash.unwrap();
        fixture
            .transfers
            .answer(fixture.client, asking.job, clash.id, nova_protocol::ClashAnswer::Merge, false)
            .unwrap();
        fixture.ended(TransferState::Done);
        assert!(!fixture.exists("from/Photos"), "the merged folder went");

        assert_eq!(fixture.run(Way::Undo), Ok(true));
        assert!(fixture.exists("from/Photos/a.png") && fixture.exists("from/notes.txt"));
        assert!(fixture.exists("to/Photos/b.png") && !fixture.exists("to/Photos/a.png"), "what was there stays");
        assert_eq!(fixture.run(Way::Redo), Ok(true));
        assert!(fixture.exists("to/Photos/a.png") && fixture.exists("to/notes.txt"));
        assert_eq!(fixture.run(Way::Undo), Ok(true));
        assert!(fixture.exists("from/Photos/a.png") && !fixture.exists("to/notes.txt"));
    }

    #[test]
    fn a_trashing_in_several_requests_is_one_step() {
        let fixture = Fixture::new("trash");
        fixture.write("from/a.txt");
        fixture.write("from/b.txt");
        for (path, continues) in [("from/a.txt", false), ("from/b.txt", true)] {
            let ids = fixture.trash.trash_each(&[fixture.text(path)]).into_iter().map(Result::unwrap).collect();
            fixture.history.trashed(ids, continues);
        }
        assert_eq!(fixture.run(Way::Undo), Ok(false));
        assert!(fixture.exists("from/a.txt") && fixture.exists("from/b.txt"));
        assert_eq!(fixture.run(Way::Undo), Ok(false), "nothing more to undo");
        assert_eq!(fixture.run(Way::Redo), Ok(false));
        assert_eq!(fixture.trashed(), 2);
    }

    #[test]
    fn what_cant_be_undone_stays_to_try_again() {
        let fixture = Fixture::new("left");
        fixture.write("from/a.txt");
        fixture.write("from/b.txt");
        let ids = fixture.trash.trash_each(&[fixture.text("from/a.txt"), fixture.text("from/b.txt")]);
        fixture.history.trashed(ids.into_iter().map(Result::unwrap).collect(), false);
        // Something else took a.txt's place meanwhile.
        fixture.write("from/a.txt");
        let error = fixture.run(Way::Undo).unwrap_err();
        assert!(error.contains("already exists"), "{error}");
        assert!(fixture.exists("from/b.txt"));
        assert_eq!(fixture.shown(), json!({"undo": "trash", "redo": "trash"}));
        std::fs::remove_file(fixture.dir.join("from/a.txt")).unwrap();
        assert_eq!(fixture.run(Way::Undo), Ok(false));
        assert_eq!(fixture.trashed(), 0);
    }

    #[test]
    fn a_new_step_ends_what_could_be_redone() {
        let fixture = Fixture::new("new");
        fixture.write("from/a.txt");
        fixture.transfer(TransferMode::Copy, &["from/a.txt"], "to");
        assert_eq!(fixture.run(Way::Undo), Ok(false));
        fixture.transfer(TransferMode::Move, &["from/a.txt"], "to");
        assert_eq!(fixture.shown(), json!({"undo": "move"}));
        assert_eq!(fixture.run(Way::Redo), Ok(false), "nothing to redo");
        assert!(Path::new(&fixture.text("to/a.txt")).exists());
    }
}
