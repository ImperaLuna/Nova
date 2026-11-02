//! Opening files in apps, for Nova Files: what type a file is (the shared
//! MIME database, `crate::mime`), which app opens it by default and which
//! others can (`associations`), starting one with the file (`launch`), and
//! making an app the new default for that type. Paths come from the
//! socket as wire text (`nova_protocol::path_to_wire`), so a name that
//! isn't UTF-8 opens too.
//!
//! The MIME database is its owner's, kept up to date (Nova Files shares it
//! with its folder listing). The
//! `mimeapps.list` files are read on every request, so a default another
//! app or the user changed counts at once, without watching them; they're
//! a few kilobytes.

use super::AppIndex;
use super::associations::{self, Associations, Places};
use super::desktop_entry::Entry;
use super::launch::{self, Runner};
use crate::logger::Logger;
use crate::mime::Mime;
use nova_protocol::{OpenWith, wire_path};
use std::path::{Component, Path};
use std::sync::{Arc, Mutex};

const LOG_SOURCE: &str = "files";

pub struct Opener {
    mime: Arc<Mime>,
    places: Places,
    apps: Arc<AppIndex>,
    runner: Runner,
    path_var: String,
    logger: Arc<Logger>,
    /// Held while a default is written, so two at once don't each write
    /// over what the other read.
    writing: Mutex<()>,
}

impl Opener {
    /// Types files with `mime`; the lists are in `places`.
    pub fn new(
        mime: Arc<Mime>,
        places: Places,
        runner: Runner,
        apps: Arc<AppIndex>,
        path_var: &str,
        logger: Arc<Logger>,
    ) -> Self {
        Self { mime, places, apps, runner, path_var: path_var.to_string(), logger, writing: Mutex::new(()) }
    }

    /// Opens `path` in its type's default app.
    pub fn open(&self, path: &str) -> Result<(), String> {
        let path = &wire_path(path);
        let types = self.types_of(path)?;
        let entry = {
            let state = self.apps.read();
            Associations::load(&self.places)
                .default_app(&types, &state)
                .cloned()
                .ok_or_else(|| format!("no app opens {}", types[0]))?
        };
        self.launch(&entry, path)
    }

    /// Opens `path` in the app `id`.
    pub fn open_with(&self, path: &str, id: &str) -> Result<(), String> {
        let path = &wire_path(path);
        self.types_of(path)?;
        let entry = self.apps.read().any_entry(id).cloned().ok_or_else(|| format!("{id} isn't installed"))?;
        self.launch(&entry, path)
    }

    /// The apps that can open `path`, the default first. Apps the launcher
    /// doesn't list only show when they're the default.
    pub fn choices(&self, path: &str) -> Result<OpenWith, String> {
        let types = self.types_of(&wire_path(path))?;
        let associations = Associations::load(&self.places);
        let state = self.apps.read();
        let default = associations.default_app(&types, &state);
        let others = associations.apps_for(&types, &state).into_iter().filter(|entry| entry.listed);
        let mut apps: Vec<&Entry> = default.into_iter().collect();
        apps.extend(others.filter(|entry| default.is_none_or(|default| !default.same_app(entry))));
        Ok(OpenWith {
            mime: types[0].clone(),
            default: default.map(|entry| entry.id.clone()),
            apps: apps.into_iter().map(|entry| state.app_of(entry)).collect(),
        })
    }

    /// Makes the app `id` the default for files of `path`'s type, and
    /// checks that it is now: a list with more say that something else
    /// keeps may still name another.
    pub fn set_default(&self, path: &str, id: &str) -> Result<(), String> {
        let types = self.types_of(&wire_path(path))?;
        if self.apps.read().any_entry(id).is_none() {
            return Err(format!("{id} isn't installed"));
        }
        let _writing = crate::lock(&self.writing);
        let written = associations::set_default(&self.places, &types[0], id)?;
        let state = self.apps.read();
        let now = Associations::load(&self.places).default_app(&types, &state).map(|entry| entry.id.clone());
        if now.as_deref() != Some(id) {
            return Err(format!(
                "wrote {}, but another list with more say still picks {}",
                written.display(),
                now.unwrap_or_default()
            ));
        }
        self.logger.info(LOG_SOURCE, &format!("{id} now opens {} ({})", types[0], written.display()));
        Ok(())
    }

    /// `path`'s type and those it's a kind of, the nearest first. The
    /// path comes from the socket: it must be absolute, without `..`.
    fn types_of(&self, path: &Path) -> Result<Vec<String>, String> {
        let plain = |part: Component| matches!(part, Component::RootDir | Component::Normal(_));
        if !path.is_absolute() || !path.components().all(plain) {
            return Err(format!("{} isn't an absolute path", path.display()));
        }
        let db = self.mime.current();
        let mime = db.type_of(path).map_err(|e| format!("can't open {}: {e}", path.display()))?;
        Ok(db.with_parents(&mime))
    }

    fn launch(&self, entry: &Entry, path: &Path) -> Result<(), String> {
        launch::launch_with_file(entry, path, &self.runner, &self.path_var, &self.logger)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::{TempDir, wait_until};
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(5);

    /// A machine with a MIME database, apps under `share`, a config folder
    /// for the lists and some files to open.
    struct Machine {
        dir: TempDir,
        opener: Opener,
        apps: Arc<AppIndex>,
    }

    fn app(share: &Path, id: &str, name: &str, extra: &str) {
        let applications = share.join("applications");
        std::fs::create_dir_all(&applications).unwrap();
        let text = format!("[Desktop Entry]\nType=Application\nName={name}\nExec={name} %f\n{extra}");
        std::fs::write(applications.join(id), text).unwrap();
    }

    fn machine(name: &str, apps: &[(&str, &str, &str)]) -> Machine {
        let dir = crate::mime::tests::data_dir(name);
        let share = dir.to_path_buf();
        for (id, name, extra) in apps {
            app(&share, id, name, extra);
        }
        let logger =
            Arc::new(Logger::open(&dir.join("log"), nova_protocol::LogLevel::Info, false, Arc::default()).unwrap());
        let index = AppIndex::of(Some(&share), &logger);
        wait_until(WAIT, "the apps to be read", || index.read().scanned());
        let places = Places::new(Some(dir.join("config")), &[], std::slice::from_ref(&share), &["niri".into()]);
        let mime = Mime::follow(vec![share.clone()], Arc::clone(&logger));
        let opener = Opener::new(mime, places, Runner::Direct, Arc::clone(&index), "", logger);
        std::fs::create_dir_all(dir.join("files")).unwrap();
        for (file, bytes) in [("photo.png", &b"\x89PNG"[..]), ("notes.md", b"# hi\n"), ("script.py", b"print()\n")] {
            std::fs::write(dir.join("files").join(file), bytes).unwrap();
        }
        Machine { dir, opener, apps: index }
    }

    impl Machine {
        fn file(&self, name: &str) -> String {
            self.dir.join("files").join(name).to_string_lossy().into_owned()
        }

        fn ids(&self, name: &str) -> Vec<String> {
            self.opener.choices(&self.file(name)).unwrap().apps.into_iter().map(|app| app.id).collect()
        }

        fn default(&self, name: &str) -> Option<String> {
            self.opener.choices(&self.file(name)).unwrap().default
        }

        fn list(&self, file: &str, text: &str) {
            std::fs::create_dir_all(self.dir.join("config")).unwrap();
            std::fs::write(self.dir.join("config").join(file), text).unwrap();
        }
    }

    #[test]
    fn a_file_opens_in_what_its_type_and_parents_name() {
        let machine = machine(
            "opening-defaults",
            &[
                ("viewer.desktop", "viewer", "MimeType=image/png;\n"),
                ("gimp.desktop", "gimp", "MimeType=image/png;image/jpeg;\n"),
                ("editor.desktop", "editor", "MimeType=text/plain;\n"),
                ("ide.desktop", "ide", "MimeType=text/x-python;\n"),
                ("hidden-viewer.desktop", "hidden", "NoDisplay=true\nMimeType=image/png;\n"),
                ("terminal.desktop", "terminal", ""),
            ],
        );
        // Nothing named: the first app that opens it, its own type's before its parents'.
        assert_eq!(machine.ids("photo.png"), ["gimp.desktop", "viewer.desktop"]);
        assert_eq!(machine.ids("script.py"), ["ide.desktop", "editor.desktop"]);
        assert_eq!(machine.ids("notes.md"), ["editor.desktop"], "markdown is a kind of plain text");
        assert_eq!(machine.opener.choices(&machine.file("notes.md")).unwrap().mime, "text/markdown");

        // A named default wins, even for a parent type over a child's own apps.
        machine
            .list("mimeapps.list", "[Default Applications]\nimage/png=viewer.desktop;\ntext/plain=editor.desktop;\n");
        assert_eq!(machine.default("photo.png").as_deref(), Some("viewer.desktop"));
        assert_eq!(machine.ids("photo.png"), ["viewer.desktop", "gimp.desktop"]);
        assert_eq!(machine.default("script.py").as_deref(), Some("editor.desktop"));

        // The desktop's own list has more say; an unlisted app shows only as the default.
        machine
            .list("niri-mimeapps.list", "[Default Applications]\nimage/png=missing.desktop;hidden-viewer.desktop;\n");
        assert_eq!(machine.ids("photo.png"), ["hidden-viewer.desktop", "gimp.desktop", "viewer.desktop"]);

        // Removed associations take apps away; added ones add apps that don't say.
        machine.list(
            "niri-mimeapps.list",
            "[Added Associations]\nimage/png=terminal.desktop;\n[Removed Associations]\nimage/png=gimp.desktop;\n",
        );
        assert_eq!(machine.ids("photo.png"), ["viewer.desktop", "terminal.desktop"]);
        // A list that adds and removes the same app removes it (viewer is
        // still mimeapps.list's default).
        machine.list(
            "niri-mimeapps.list",
            "[Added Associations]\nimage/png=terminal.desktop;\n[Removed Associations]\nimage/png=terminal.desktop;\n",
        );
        assert_eq!(machine.ids("photo.png"), ["viewer.desktop", "gimp.desktop"]);
        // A default a list with more say removed from the type is skipped.
        machine.list("niri-mimeapps.list", "[Removed Associations]\nimage/png=viewer.desktop;\n");
        assert_eq!(machine.default("photo.png").as_deref(), Some("gimp.desktop"));
        drop(machine.apps);
    }

    #[test]
    fn a_new_default_sticks_and_others_are_kept() {
        let machine = machine(
            "opening-set-default",
            &[("viewer.desktop", "viewer", "MimeType=image/png;\n"), ("gimp.desktop", "gimp", "MimeType=image/png;\n")],
        );
        machine.list("mimeapps.list", "[Default Applications]\ntext/plain=editor.desktop;\n");
        machine.opener.set_default(&machine.file("photo.png"), "viewer.desktop").unwrap();
        assert_eq!(machine.default("photo.png").as_deref(), Some("viewer.desktop"));
        let written = std::fs::read_to_string(machine.dir.join("config/mimeapps.list")).unwrap();
        assert!(written.contains("text/plain=editor.desktop;"), "{written}");
        assert!(machine.opener.set_default(&machine.file("photo.png"), "missing.desktop").is_err());

        // A list with more say that can't be written still names another.
        let store = machine.dir.join("store.list");
        std::fs::write(&store, "[Default Applications]\nimage/png=gimp.desktop;\n").unwrap();
        std::os::unix::fs::symlink(&store, machine.dir.join("config/niri-mimeapps.list")).unwrap();
        let error = machine.opener.set_default(&machine.file("photo.png"), "viewer.desktop").unwrap_err();
        assert!(error.contains("gimp.desktop"), "{error}");
    }

    #[test]
    fn opening_starts_the_default_app_with_the_file() {
        let machine = machine("opening-launch", &[]);
        let bin = machine.dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let opened = machine.dir.join("opened");
        let program =
            nova_test_support::write_program(&bin, "record", &format!("printf '%s' \"$1\" > {}", opened.display()));
        app(&machine.dir, "record.desktop", &program.to_string_lossy(), "MimeType=image/png;\n");
        // The index sees the new app by itself.
        wait_until(WAIT, "the new app", || machine.apps.read().any_entry("record.desktop").is_some());
        let file = machine.dir.join("files/photo.png").to_string_lossy().into_owned();
        let opener = machine.opener;
        opener.open(&file).unwrap();
        wait_until(WAIT, "the app to get the file", || std::fs::read_to_string(&opened).is_ok_and(|got| got == file));
        assert!(opener.open("relative/photo.png").is_err());
        assert!(opener.open(&format!("{}/../files/photo.png", machine.dir.join("files").display())).is_err());
        assert!(opener.open(&machine.dir.join("files/missing.png").to_string_lossy()).is_err());
        assert!(opener.open_with(&file, "missing.desktop").is_err());
    }

    /// A file named in Latin-1, as the listing sends it, reaches the app
    /// byte for byte.
    #[test]
    fn a_name_that_isnt_utf8_opens() {
        use std::os::unix::ffi::OsStrExt;
        let machine = machine("opening-latin1", &[]);
        let bin = machine.dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let opened = machine.dir.join("opened");
        let program =
            nova_test_support::write_program(&bin, "record", &format!("printf '%s' \"$1\" > {}", opened.display()));
        app(&machine.dir, "record.desktop", &program.to_string_lossy(), "MimeType=image/png;\n");
        wait_until(WAIT, "the new app", || machine.apps.read().any_entry("record.desktop").is_some());
        let file = machine.dir.join("files").join(std::ffi::OsStr::from_bytes(b"caf\xe9 $1.png"));
        std::fs::write(&file, b"\x89PNG").unwrap();
        let wire = nova_protocol::wire_text(&file);
        assert_eq!(machine.opener.choices(&wire).unwrap().mime, "image/png");
        machine.opener.open(&wire).unwrap();
        let sent = file.as_os_str().as_bytes();
        wait_until(WAIT, "the app to get the file", || std::fs::read(&opened).is_ok_and(|got| got == sent));
    }
}
