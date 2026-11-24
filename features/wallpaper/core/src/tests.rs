use super::*;
use nova_hub::Hub;
use nova_test_support::{TempDir, eventually, wait_until};

const WAIT: Duration = Duration::from_secs(10);
const WORKSHOP_ID: &str = "2861349227";

struct Fixture {
    dir: TempDir,
    logger: Arc<Logger>,
    path: String,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = TempDir::new(&format!("wallpaper-{name}"));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let runs = dir.join("runs");
        nova_test_support::write_program(
            &dir.join("bin"),
            DAEMON,
            &format!("echo $$ >> '{}'\nexec sleep 30", runs.display()),
        );
        nova_test_support::write_program(
            &dir.join("bin"),
            CLIENT,
            &format!(
                "[ \"$1\" = img ] && [ -e '{}' ] && exit 1\necho \"$*\" >> '{}'\n\
                 [ \"$1\" = query ] && [ -e '{query}' ] && cat '{query}'\nexit 0",
                dir.join("fail").display(),
                dir.join("awww").display(),
                query = dir.join("query").display(),
            ),
        );
        nova_test_support::write_program(
            &dir.join("bin"),
            engine::PROGRAM,
            &format!(
                "echo $$ \"$*\" >> '{}'\nwhile [ $# -gt 0 ]; do\n\
                 [ \"$1\" = --screenshot ] && [ -e '{frame}' ] && cp '{frame}' \"$2\"\nshift\ndone\n\
                 exec sleep 30",
                dir.join("engine").display(),
                frame = dir.join("frame.png").display(),
            ),
        );
        let item = dir.join(format!("home/.local/share/Steam/steamapps/workshop/content/431960/{WORKSHOP_ID}"));
        std::fs::create_dir_all(&item).unwrap();
        std::fs::write(item.join(source::PROJECT_FILE), r#"{"type": "scene", "preview": "preview.png"}"#).unwrap();
        for image in [dir.join("dusk.png"), dir.join("tide.png"), item.join("preview.png")] {
            let file = std::fs::File::create(image).unwrap();
            let mut encoder = png::Encoder::new(file, 1, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.write_header().unwrap().write_image_data(&[0, 0, 0]).unwrap();
        }
        std::fs::write(dir.join("rain.webm"), "frames").unwrap();
        let sizes =
            ["DP-1", "DP-2"].map(|name| format!("nova: {name}: 1x1, scale: 1, currently displaying: color: 0\n"));
        std::fs::write(dir.join("query"), sizes.concat()).unwrap();
        let logger =
            Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::new(Hub::default())).unwrap());
        let path = nova_test_support::fake_path(&dir.join("bin"), &["sleep", "cp", "cat"]);
        Self { dir, logger, path }
    }

    fn start(&self) -> Arc<Wallpaper> {
        Wallpaper::start(
            &self.dir.join("state"),
            Some(self.dir.join("home")),
            self.path.clone(),
            true,
            Arc::clone(&self.logger),
        )
    }

    fn image(&self, name: &str) -> String {
        self.dir.join(name).display().to_string()
    }

    fn config(&self, name: &str) -> WallpaperConfig {
        WallpaperConfig { path: Some(self.image(name)), ..WallpaperConfig::default() }
    }

    fn runs(&self, count: usize) -> Vec<i32> {
        eventually(WAIT, &format!("{DAEMON} to start {count} times"), || {
            let runs: Vec<i32> = std::fs::read_to_string(self.dir.join("runs"))
                .unwrap_or_default()
                .lines()
                .map(|line| line.parse().unwrap())
                .collect();
            (runs.len() >= count).then_some(runs)
        })
    }

    fn images(&self, count: usize) -> Vec<(String, String)> {
        eventually(WAIT, &format!("{count} images sent"), || {
            let sent = self.sent();
            (sent.len() >= count).then_some(sent)
        })
    }

    fn shows(&self, expected: &[(&str, &str)]) {
        wait_until(WAIT, &format!("outputs to show {expected:?}"), || {
            let sent = self.sent();
            let last: BTreeMap<&str, &str> = sent
                .iter()
                .flat_map(|(outputs, image)| outputs.split(',').map(move |output| (output, image.as_str())))
                .collect();
            expected.iter().all(|(output, image)| last.get(*output) == Some(image))
        });
    }

    fn item(&self) -> String {
        let workshop = "home/.local/share/Steam/steamapps/workshop/content/431960";
        self.dir.join(workshop).join(WORKSHOP_ID).display().to_string()
    }

    fn engine_runs(&self) -> Vec<(i32, Vec<(String, String)>)> {
        std::fs::read_to_string(self.dir.join("engine"))
            .unwrap_or_default()
            .lines()
            .map(|line| {
                let words: Vec<&str> = line.split(' ').collect();
                let screens = words
                    .windows(4)
                    .filter(|four| four[0] == "--screen-root" && four[2] == "--bg")
                    .map(|four| (four[1].to_owned(), four[3].to_owned()))
                    .collect();
                (words[0].parse().unwrap(), screens)
            })
            .collect()
    }

    fn engine_shows(&self, count: usize, expected: &[(&str, &str)]) -> i32 {
        let expected: Vec<(String, String)> =
            expected.iter().map(|(output, project)| ((*output).to_owned(), (*project).to_owned())).collect();
        eventually(WAIT, &format!("{} to show {expected:?}", engine::PROGRAM), || {
            let runs = self.engine_runs();
            let (last, screens) = runs.last().filter(|_| runs.len() >= count)?;
            let others_gone = runs[..runs.len() - 1].iter().all(|(pid, _)| !alive(*pid));
            (*screens == expected && alive(*last) && others_gone).then_some(*last)
        })
    }

    fn sent(&self) -> Vec<(String, String)> {
        std::fs::read_to_string(self.dir.join("awww"))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.starts_with("img "))
            .map(|line| {
                let words: Vec<&str> = line.split(' ').collect();
                let outputs = words.iter().position(|word| *word == "--outputs").map(|at| words[at + 1]);
                (outputs.unwrap_or("every").to_owned(), words.last().unwrap().to_string())
            })
            .collect()
    }
}

fn alive(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
        && !std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default().contains(") Z ")
}

fn outputs(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn nothing_runs_until_an_image_is_set() {
    let fixture = Fixture::new("unset");
    let wallpaper = fixture.start();
    wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
    wallpaper.configure(true, false, &WallpaperConfig::default());
    nova_test_support::settle(Duration::from_millis(300));
    assert!(!fixture.dir.join("runs").exists(), "a wallpaper tool of the user's keeps the screen");
    wallpaper.set(&fixture.image("dusk.png"), None).unwrap();
    fixture.runs(1);
    assert_eq!(fixture.images(1), [("DP-1,DP-2".into(), fixture.image("dusk.png"))]);
    let line = std::fs::read_to_string(fixture.dir.join("awww")).unwrap();
    assert!(line.contains("--namespace nova") && line.contains("--transition-type fade"), "{line}");
    wallpaper.stop();
}

#[test]
fn only_outputs_that_change_get_an_image() {
    let fixture = Fixture::new("changes");
    let wallpaper = fixture.start();
    wallpaper.configure(true, false, &fixture.config("dusk.png"));
    nova_test_support::settle(Duration::from_millis(300));
    assert!(fixture.sent().is_empty(), "nothing before the outputs are known");
    wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
    let (dusk, tide) = (fixture.image("dusk.png"), fixture.image("tide.png"));
    assert_eq!(fixture.images(1), [("DP-1,DP-2".into(), dusk.clone())], "one fade for both");
    wallpaper.set(&tide, Some("DP-2")).unwrap();
    fixture.shows(&[("DP-1", &dusk), ("DP-2", &tide)]);
    wallpaper.outputs(outputs(&["DP-1", "DP-2", "HDMI-A-1"]));
    fixture.shows(&[("DP-1", &dusk), ("DP-2", &tide), ("HDMI-A-1", &dusk)]);
    let sent = fixture.sent().len();
    wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
    wallpaper.outputs(outputs(&["DP-1", "DP-2", "HDMI-A-1"]));
    wait_until(WAIT, "HDMI-A-1 to get its image again", || {
        fixture.sent()[sent..]
            .iter()
            .any(|(again, image)| again.split(',').any(|output| output == "HDMI-A-1") && *image == dusk)
    });
    assert_eq!(wallpaper.current().unwrap().outputs["DP-2"], tide);
    assert_eq!(fixture.runs(1).len(), 1);
    wallpaper.stop();
}

#[test]
fn the_choice_lasts_until_the_config_changes() {
    let fixture = Fixture::new("saved");
    let config = fixture.config("dusk.png");
    let first = fixture.start();
    first.configure(true, false, &config);
    first.set(&fixture.image("tide.png"), None).unwrap();
    first.stop();
    let again = fixture.start();
    again.configure(true, false, &config);
    assert_eq!(again.current().unwrap().path, Some(fixture.image("tide.png")));
    let edited = WallpaperConfig { outputs: [("DP-2".into(), fixture.image("tide.png"))].into(), ..config };
    again.configure(true, false, &edited);
    assert_eq!(again.current().unwrap().path, Some(fixture.image("dusk.png")));
    assert!(!fixture.dir.join("state").join(STATE_FILE).exists());
    again.stop();
    let last = fixture.start();
    last.configure(true, false, &edited);
    assert_eq!(last.current().unwrap().path, Some(fixture.image("dusk.png")));
    last.stop();
}

#[test]
fn a_new_folder_keeps_the_choice() {
    let fixture = Fixture::new("folder");
    let config = fixture.config("dusk.png");
    let first = fixture.start();
    first.configure(true, false, &config);
    first.set(&fixture.image("tide.png"), None).unwrap();
    let moved = WallpaperConfig { folder: Some("/art".into()), ..config };
    first.configure(true, false, &moved);
    assert_eq!(first.current().unwrap().path, Some(fixture.image("tide.png")));
    first.stop();
    let again = fixture.start();
    again.configure(true, false, &moved);
    assert_eq!(again.current().unwrap().path, Some(fixture.image("tide.png")));
    again.stop();
}

#[test]
fn the_config_changing_while_novad_is_off_wins() {
    let fixture = Fixture::new("offline-edit");
    let first = fixture.start();
    first.configure(true, false, &fixture.config("dusk.png"));
    first.set(&fixture.image("tide.png"), None).unwrap();
    first.stop();
    let again = fixture.start();
    again.configure(true, false, &WallpaperConfig::default());
    assert_eq!(again.current(), Ok(Wallpapers::default()));
    again.stop();
}

#[test]
fn choosing_the_same_file_again_sends_it_again() {
    let fixture = Fixture::new("again");
    let wallpaper = fixture.start();
    wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
    wallpaper.configure(true, false, &WallpaperConfig::default());
    let tide = fixture.image("tide.png");
    wallpaper.set(&tide, Some("DP-2")).unwrap();
    fixture.images(1);
    wallpaper.set(&tide, Some("DP-2")).unwrap();
    assert_eq!(fixture.images(2)[1], ("DP-2".into(), tide.clone()));
    wallpaper.set(&tide, None).unwrap();
    assert_eq!(fixture.images(3)[2], ("DP-1,DP-2".into(), tide));
    wallpaper.stop();
}

#[test]
fn a_failed_image_is_sent_again() {
    let fixture = Fixture::new("resend");
    std::fs::write(fixture.dir.join("fail"), "").unwrap();
    let wallpaper = fixture.start();
    wallpaper.outputs(outputs(&["DP-1"]));
    wallpaper.configure(true, false, &fixture.config("dusk.png"));
    fixture.runs(1);
    nova_test_support::settle(Duration::from_millis(300));
    std::fs::remove_file(fixture.dir.join("fail")).unwrap();
    fixture.shows(&[("DP-1", &fixture.image("dusk.png"))]);
    assert_eq!(fixture.runs(1).len(), 1, "the daemon is fine");
    wallpaper.stop();
}

#[test]
fn a_crashed_daemon_starts_again_and_gets_its_images_back() {
    let fixture = Fixture::new("crash");
    let wallpaper = fixture.start();
    wallpaper.outputs(outputs(&["DP-1"]));
    wallpaper.configure(true, false, &fixture.config("dusk.png"));
    let first = fixture.runs(1)[0];
    fixture.images(1);
    // SAFETY: kill only sends a signal.
    unsafe { libc::kill(first, libc::SIGKILL) };
    let second = fixture.runs(2)[1];
    assert_ne!(first, second);
    assert_eq!(fixture.images(2)[1], ("DP-1".into(), fixture.image("dusk.png")));
    wallpaper.stop();
    wait_until(WAIT, "stopping to end it", || !alive(second));
}

#[test]
fn the_daemon_stops_with_the_feature() {
    let fixture = Fixture::new("off");
    let wallpaper = fixture.start();
    wallpaper.outputs(outputs(&["DP-1"]));
    wallpaper.configure(true, false, &fixture.config("dusk.png"));
    let pid = fixture.runs(1)[0];
    wallpaper.configure(false, false, &fixture.config("dusk.png"));
    wait_until(WAIT, "the daemon to stop", || !alive(pid));
    assert!(wallpaper.set(&fixture.image("tide.png"), None).unwrap_err().contains("isn't ready"));
    nova_test_support::settle(Restarts::FIRST + Duration::from_millis(200));
    assert_eq!(fixture.runs(1).len(), 1, "not started again while off");
    wallpaper.stop();
}

#[test]
fn a_bad_choice_is_refused_and_changes_nothing() {
    let fixture = Fixture::new("refused");
    let wallpaper = fixture.start();
    wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
    wallpaper.configure(true, false, &fixture.config("dusk.png"));
    assert!(wallpaper.set("dusk.png", None).unwrap_err().contains("absolute"));
    assert!(wallpaper.set(&fixture.image("missing.jpg"), None).unwrap_err().contains("can't read"));
    let folder = wallpaper.set(&fixture.dir.display().to_string(), None).unwrap_err();
    assert!(folder.contains("isn't a Wallpaper Engine project"), "{folder}");
    std::fs::write(fixture.dir.join("notes.toml"), "path = \"dusk.png\"").unwrap();
    assert!(wallpaper.set(&fixture.image("notes.toml"), None).unwrap_err().contains("isn't an image, a video"));
    let unknown = wallpaper.set(&fixture.image("tide.png"), Some("HDMI-A-1")).unwrap_err();
    assert!(unknown.contains("DP-1, DP-2"), "{unknown}");
    assert!(wallpaper.set(&fixture.image("tide.png"), Some("DP-1,DP-2")).is_err());
    assert_eq!(wallpaper.current().unwrap().path, Some(fixture.image("dusk.png")));
    assert!(!fixture.dir.join("state").join(STATE_FILE).exists());
    wallpaper.stop();
}

#[test]
fn a_dynamic_wallpaper_runs_over_its_preview() {
    let fixture = Fixture::new("dynamic");
    let wallpaper = fixture.start();
    wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
    wallpaper.configure(true, true, &WallpaperConfig { path: Some(WORKSHOP_ID.into()), ..Default::default() });
    let (item, preview) = (fixture.item(), format!("{}/preview.png", fixture.item()));
    fixture.shows(&[("DP-1", &preview), ("DP-2", &preview)]);
    let first = fixture.engine_shows(1, &[("DP-1", &item), ("DP-2", &item)]);
    assert_eq!(wallpaper.current().unwrap().path.as_deref(), Some(WORKSHOP_ID));
    let tide = fixture.image("tide.png");
    wallpaper.set(&tide, Some("DP-2")).unwrap();
    fixture.shows(&[("DP-1", &preview), ("DP-2", &tide)]);
    let second = fixture.engine_shows(2, &[("DP-1", &item)]);
    assert_ne!(first, second);
    wallpaper.set(&fixture.image("dusk.png"), None).unwrap();
    wait_until(WAIT, "the engine to end", || !alive(second));
    nova_test_support::settle(Restarts::FIRST + Duration::from_millis(200));
    assert_eq!(fixture.engine_runs().len(), 2, "asked to end isn't a crash");
    assert_eq!(fixture.runs(1).len(), 1, "awww ran all along");
    wallpaper.stop();
}

#[test]
fn a_dynamic_wallpaper_starts_from_its_last_frame_next_time() {
    let fixture = Fixture::new("stills");
    std::fs::create_dir_all(fixture.dir.join("home/.local/share/Steam/steamapps/common/wallpaper_engine/assets"))
        .unwrap();
    let query = |dp1: &str| {
        format!(
            "nova: DP-1: {dp1}, scale: 1, currently displaying: image: x\nnova: DP-2: 1x1, scale: 2, currently displaying: image: x\n"
        )
    };
    std::fs::write(fixture.dir.join("query"), query("3x2")).unwrap();
    let file = std::fs::File::create(fixture.dir.join("frame.png")).unwrap();
    let mut encoder = png::Encoder::new(file, 5, 2);
    encoder.set_color(png::ColorType::Rgb);
    encoder.write_header().unwrap().write_image_data(&[9; 5 * 2 * 3]).unwrap();
    let config = WallpaperConfig { path: Some(WORKSHOP_ID.into()), ..Default::default() };
    let start = || {
        let wallpaper = fixture.start();
        wallpaper.outputs(outputs(&["DP-1", "DP-2"]));
        wallpaper.configure(true, true, &config);
        wallpaper
    };
    let (item, preview) = (fixture.item(), format!("{}/preview.png", fixture.item()));
    let engine_folder = fixture.dir.join("state").join("wallpaper-engine");
    let still = |size| engine::still(Path::new(&item), size, &engine_folder);
    let first = start();
    fixture.shows(&[("DP-1", &preview), ("DP-2", &preview)]);
    fixture.engine_shows(1, &[("DP-1", &item), ("DP-2", &item)]);
    wait_until(WAIT, "the frame to be kept", || still((3, 2)).is_some() && still((2, 2)).is_some());
    first.stop();
    let sent = fixture.sent().len();
    let again = start();
    let (big, small) = (still((3, 2)).unwrap().display().to_string(), still((2, 2)).unwrap().display().to_string());
    fixture.shows(&[("DP-1", &big), ("DP-2", &small)]);
    assert!(fixture.sent()[sent..].iter().all(|(_, image)| *image != preview), "no preview first");
    again.stop();
    std::fs::write(fixture.dir.join("query"), query("6x4")).unwrap();
    let bigger = start();
    fixture.shows(&[("DP-1", &preview), ("DP-2", &small)]);
    bigger.set(&fixture.image("dusk.png"), None).unwrap();
    wait_until(WAIT, "the unused stills to go", || !engine::has_stills(Path::new(&item), &engine_folder));
    bigger.stop();
}

#[test]
fn a_video_on_two_outputs_gets_stills_too() {
    let fixture = Fixture::new("video-stills");
    let file = std::fs::File::create(fixture.dir.join("frame.png")).unwrap();
    let mut encoder = png::Encoder::new(file, 2, 1);
    encoder.set_color(png::ColorType::Rgb);
    encoder.write_header().unwrap().write_image_data(&[9; 2 * 3]).unwrap();
    let config = WallpaperConfig { path: Some(fixture.image("rain.webm")), ..Default::default() };
    let first = fixture.start();
    first.outputs(outputs(&["DP-1", "DP-2"]));
    first.configure(true, true, &config);
    let project = eventually(WAIT, "the engine to start", || fixture.engine_runs().first()?.1.first().cloned()).1;
    let engine_folder = fixture.dir.join("state").join("wallpaper-engine");
    wait_until(WAIT, "the frame to be kept", || engine::still(Path::new(&project), (1, 1), &engine_folder).is_some());
    assert!(fixture.sent().is_empty(), "nothing to show the first time");
    first.stop();
    let again = fixture.start();
    again.outputs(outputs(&["DP-1", "DP-2"]));
    again.configure(true, true, &config);
    let still = engine::still(Path::new(&project), (1, 1), &engine_folder).unwrap().display().to_string();
    fixture.shows(&[("DP-1", &still), ("DP-2", &still)]);
    again.stop();
}

#[test]
fn awww_says_each_outputs_size_in_pixels() {
    let answer = "nova: DP-1: 2560x1440, scale: 1, currently displaying: image: /a: b.png\n\
                  eDP-1: 1280x800, scale: 1.25, currently displaying: color: 000000\n\
                  nova: HDMI-A-1: 0x0, scale: 1, currently displaying: color: 000000\nodd line";
    let sizes = sizes(answer);
    assert_eq!(sizes, [("DP-1".to_owned(), (2560, 1440)), ("eDP-1".to_owned(), (1600, 1000))].into());
}

#[test]
fn the_engine_starts_again_after_a_crash_and_after_awww_does() {
    let fixture = Fixture::new("engine-crash");
    let wallpaper = fixture.start();
    wallpaper.outputs(outputs(&["DP-1"]));
    wallpaper.configure(true, true, &WallpaperConfig::default());
    wallpaper.set(WORKSHOP_ID, None).unwrap();
    let item = fixture.item();
    let first = fixture.engine_shows(1, &[("DP-1", &item)]);
    // SAFETY: kill only sends a signal.
    unsafe { libc::kill(first, libc::SIGKILL) };
    let second = fixture.engine_shows(2, &[("DP-1", &item)]);
    assert_ne!(first, second);
    let daemon = fixture.runs(1)[0];
    // SAFETY: kill only sends a signal.
    unsafe { libc::kill(daemon, libc::SIGKILL) };
    fixture.runs(2);
    let third = fixture.engine_shows(3, &[("DP-1", &item)]);
    assert_ne!(second, third);
    wallpaper.stop();
    wait_until(WAIT, "stopping to end it", || !alive(third));
}

#[test]
fn a_video_plays_through_a_project_of_its_own() {
    let fixture = Fixture::new("video");
    let wallpaper = fixture.start();
    wallpaper.outputs(outputs(&["DP-1"]));
    wallpaper.configure(true, true, &WallpaperConfig::default());
    wallpaper.set(&fixture.image("rain.webm"), None).unwrap();
    let project = eventually(WAIT, "the engine to start", || fixture.engine_runs().first()?.1.first().cloned()).1;
    let videos = fixture.dir.join("state").join("wallpaper-engine").join("videos");
    assert!(Path::new(&project).starts_with(&videos), "{project}");
    assert!(Path::new(&project).join(source::PROJECT_FILE).is_file());
    assert!(fixture.sent().is_empty(), "a video has no preview for awww");
    wallpaper.set(&fixture.image("dusk.png"), None).unwrap();
    fixture.shows(&[("DP-1", &fixture.image("dusk.png"))]);
    wait_until(WAIT, "the unused project to go", || !Path::new(&project).exists());
    wallpaper.stop();
}

#[test]
fn dynamic_wallpapers_need_their_feature() {
    let fixture = Fixture::new("engine-off");
    let wallpaper = fixture.start();
    wallpaper.outputs(outputs(&["DP-1"]));
    wallpaper.configure(true, false, &WallpaperConfig { path: Some(WORKSHOP_ID.into()), ..Default::default() });
    fixture.shows(&[("DP-1", &format!("{}/preview.png", fixture.item()))]);
    let refused = wallpaper.set(&fixture.image("rain.webm"), None).unwrap_err();
    assert!(refused.contains(ENGINE_FEATURE_ID), "{refused}");
    let missing = wallpaper.set("1", None).unwrap_err();
    assert!(missing.contains("isn't downloaded"), "{missing}");
    nova_test_support::settle(Duration::from_millis(300));
    assert!(fixture.engine_runs().is_empty());
    wallpaper.configure(false, true, &WallpaperConfig::default());
    assert!(wallpaper.set(&fixture.image("dusk.png"), None).unwrap_err().contains("isn't ready"));
    wallpaper.set(WORKSHOP_ID, None).unwrap();
    fixture.engine_shows(1, &[("DP-1", &fixture.item())]);
    wallpaper.stop();
}

#[test]
fn images_come_back_after_awww_was_ended() {
    let fixture = Fixture::new("ended");
    let wallpaper = fixture.start();
    wallpaper.outputs(outputs(&["DP-1"]));
    let dusk = fixture.config("dusk.png");
    wallpaper.configure(true, false, &dusk);
    let first = fixture.runs(1)[0];
    fixture.images(1);
    wallpaper.configure(false, false, &dusk);
    wait_until(WAIT, "the daemon to end", || !alive(first));
    wallpaper.configure(true, false, &dusk);
    fixture.runs(2);
    assert_eq!(fixture.images(2)[1], ("DP-1".into(), fixture.image("dusk.png")));
    wallpaper.stop();
}
