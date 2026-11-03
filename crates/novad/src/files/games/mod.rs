//! The sidebar's Games place (`files_games_list`): the games installed on
//! this machine as folders, so a game's files are one click away. Read
//! anew each time a window goes there, off the connection's thread.
//!
//! Steam is the only launcher read so far (`steam.rs`, over Valve's
//! KeyValues files in `vdf.rs`). The place shows while one of the Steam
//! installs Nova knows of is there.

mod steam;
mod vdf;

use crate::logger::Logger;
use nova_protocol::Game;
use std::path::{Path, PathBuf};

/// Where Steam installs itself under Home, after `$XDG_DATA_HOME/Steam`:
/// the `~/.steam/steam` link older installs and some distros' packages
/// use, and the Flathub app's.
const STEAM_UNDER_HOME: [&str; 2] = [".steam/steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"];

/// Where the launchers keep their libraries.
#[derive(Clone)]
pub struct Sources {
    /// Where Steam may be installed, most likely first.
    pub steam: Vec<PathBuf>,
}

impl Sources {
    /// The places on this machine, for `home` with its data folder `data`.
    pub fn system(home: &Path, data: Option<PathBuf>) -> Self {
        let steam = data.map(|data| data.join("Steam")).into_iter();
        Self { steam: steam.chain(STEAM_UNDER_HOME.iter().map(|path| home.join(path))).collect() }
    }

    /// Whether a launcher is installed, so the place is worth showing.
    pub fn any(&self) -> bool {
        self.steam.iter().any(|root| root.join("steamapps").is_dir())
    }
}

/// The installed games, in name order.
pub fn list(sources: &Sources, logger: &Logger) -> Vec<Game> {
    let mut games = steam::games(&sources.steam, logger);
    games.sort_by(|a, b| super::folder::natural(&a.name, &b.name));
    games
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::Hub;
    use nova_protocol::LogLevel;
    use nova_test_support::TempDir;
    use std::sync::Arc;

    /// A home with Steam as it installs itself, its own library holding
    /// one game and Proton, a second library on another drive.
    struct Fixture {
        dir: TempDir,
        logger: Logger,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir = TempDir::new(&format!("games-{name}"));
            let logger = Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::new(Hub::default())).unwrap();
            Self { dir, logger }
        }

        fn root(&self) -> PathBuf {
            self.dir.join("home/.local/share/Steam")
        }

        fn drive(&self) -> PathBuf {
            self.dir.join("run/media/ana/GAMES/SteamLibrary")
        }

        fn sources(&self) -> Sources {
            Sources::system(&self.dir.join("home"), Some(self.dir.join("home/.local/share")))
        }

        fn write(&self, path: &Path, text: &str) {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }

        /// Installs app `app` in `library` from its recorded manifest.
        fn install(&self, library: &Path, app: &str, folder: &str) {
            let manifest = recorded(&format!("appmanifest_{app}.acf"));
            self.write(&library.join(format!("steamapps/appmanifest_{app}.acf")), &manifest);
            std::fs::create_dir_all(library.join("steamapps/common").join(folder)).unwrap();
        }

        /// Steam with both libraries and what's in them.
        fn steam(&self) {
            let list = recorded("libraryfolders.vdf")
                .replace("/home/ana/.local/share/Steam", &self.root().display().to_string())
                .replace("/run/media/ana/GAMES/SteamLibrary", &self.drive().display().to_string());
            self.write(&self.root().join("steamapps/libraryfolders.vdf"), &list);
            self.install(&self.root(), "1245620", "ELDEN RING");
            self.install(&self.root(), "2805730", "Proton 9.0");
            self.write(&self.root().join("steamapps/common/Proton 9.0/toolmanifest.vdf"), "\"manifest\" {}");
            self.install(&self.drive(), "3321460", "Crimson Desert");
            self.install(&self.drive(), "228980", "Steamworks Shared");
        }
    }

    fn recorded(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/files/recorded/steam").join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn names(games: &[Game]) -> Vec<&str> {
        games.iter().map(|game| game.name.as_str()).collect()
    }

    #[test]
    fn a_new_user_has_no_games_and_no_place() {
        let fixture = Fixture::new("none");
        assert!(!fixture.sources().any());
        assert!(list(&fixture.sources(), &fixture.logger).is_empty());
    }

    #[test]
    fn games_come_from_every_library_without_tools() {
        let fixture = Fixture::new("libraries");
        fixture.steam();
        assert!(fixture.sources().any());
        let games = list(&fixture.sources(), &fixture.logger);
        assert_eq!(names(&games), ["Crimson Desert", "ELDEN RING"], "in name order, Proton and installers left out");
        let crimson = &games[0];
        assert_eq!(crimson.folder, fixture.drive().join("steamapps/common/Crimson Desert").display().to_string());
        assert_eq!(crimson.size, Some(140_563_734_528));
        assert_eq!(crimson.saves, None);
    }

    #[test]
    fn saves_are_the_proton_prefix_else_steam_cloud() {
        let fixture = Fixture::new("saves");
        fixture.steam();
        let prefix = fixture.drive().join("steamapps/compatdata/3321460/pfx/drive_c/users/steamuser");
        std::fs::create_dir_all(&prefix).unwrap();
        let cloud = fixture.root().join("userdata/81234567/1245620");
        std::fs::create_dir_all(&cloud).unwrap();
        let games = list(&fixture.sources(), &fixture.logger);
        assert_eq!(games[0].saves, Some(prefix.display().to_string()));
        assert_eq!(games[1].saves, Some(cloud.display().to_string()));
    }

    #[test]
    fn the_same_steam_through_its_link_is_read_once() {
        let fixture = Fixture::new("link");
        fixture.steam();
        std::fs::create_dir_all(fixture.dir.join("home/.steam")).unwrap();
        std::os::unix::fs::symlink(fixture.root(), fixture.dir.join("home/.steam/steam")).unwrap();
        assert_eq!(list(&fixture.sources(), &fixture.logger).len(), 2);
    }

    #[test]
    fn a_manifest_reaching_outside_its_library_is_skipped() {
        let fixture = Fixture::new("outside");
        fixture.steam();
        let sneaky = recorded("appmanifest_1245620.acf").replace("\"ELDEN RING\"\n", "\"../../..\"\n");
        fixture.write(&fixture.root().join("steamapps/appmanifest_1245620.acf"), &sneaky);
        fixture.write(&fixture.drive().join("steamapps/appmanifest_9.acf"), "\"AppState\" {");
        assert_eq!(names(&list(&fixture.sources(), &fixture.logger)), ["Crimson Desert"]);
    }
}
