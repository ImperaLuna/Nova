use super::source::PROJECT_FILE;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PROGRAM: &str = "linux-wallpaperengine";
// On niri the default bottom layer is cloned onto every workspace in the overview, and pausing under any
// output-sized window freezes every screen: https://linear.app/imperaluna/issue/IMP-168
const FLAGS: &[&str] = &[
    "--silent",
    "--layer",
    "background",
    "--fullscreen-pause-only-active",
    "--scaling",
    "fill",
    "--screenshot-delay",
    "5",
];
const STEAM_ASSETS: &str = "steamapps/common/wallpaper_engine/assets";
const STATE_FOLDER: &str = "wallpaper-engine";
const EMPTY_ASSETS: &str = "assets";
const VIDEO_PROJECTS: &str = "videos";
const FRAME_FILE: &str = "frame.png";
const STILLS: &str = "stills";
// nixpkgs' linux-wallpaperengine misses the driver folder, so NVIDIA decodes video on the CPU. Remove once nixpkgs
// wraps it with the driver path: https://linear.app/imperaluna/issue/IMP-168
const NIXOS_DRIVER_LIBS: &str = "/run/opengl-driver/lib";

pub fn state_folder(state_dir: &Path) -> PathBuf {
    state_dir.join(STATE_FOLDER)
}

pub fn frame_file(folder: &Path) -> PathBuf {
    folder.join(FRAME_FILE)
}

pub fn command(screens: &BTreeMap<String, PathBuf>, assets: &Path, frame: &Path, path_var: &str) -> Command {
    let mut command = Command::new(PROGRAM);
    command.env("PATH", path_var).args(FLAGS).arg("--assets-dir").arg(assets).arg("--screenshot").arg(frame);
    for (output, project) in screens {
        command.arg("--screen-root").arg(output).arg("--bg").arg(project);
    }
    if Path::new(NIXOS_DRIVER_LIBS).is_dir() {
        let mut libraries = OsString::from(NIXOS_DRIVER_LIBS);
        if let Some(more) = std::env::var_os("LD_LIBRARY_PATH").filter(|more| !more.is_empty()) {
            libraries.push(":");
            libraries.push(more);
        }
        command.env("LD_LIBRARY_PATH", libraries);
    }
    command
}

pub fn assets(steam_roots: &[PathBuf], folder: &Path) -> Result<PathBuf, String> {
    if let Some(steam) = steam_roots.iter().map(|root| root.join(STEAM_ASSETS)).find(|assets| assets.is_dir()) {
        return Ok(steam);
    }
    let empty = folder.join(EMPTY_ASSETS);
    std::fs::create_dir_all(&empty).map_err(|e| format!("can't make {}: {e}", empty.display()))?;
    Ok(empty)
}

pub fn video_project(video: &Path, folder: &Path) -> Result<PathBuf, String> {
    let video = &std::fs::canonicalize(video).map_err(|e| format!("can't read {}: {e}", video.display()))?;
    let metadata = std::fs::metadata(video).map_err(|e| format!("can't read {}: {e}", video.display()))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    let stem = video.file_stem().unwrap_or_default().to_string_lossy();
    let extension = video.extension().unwrap_or_default().to_string_lossy();
    let videos = folder.join(VIDEO_PROJECTS);
    let file = format!("{}-{}", metadata.dev(), metadata.ino());
    let project = videos.join(format!("{stem}-{file}-{}-{modified}", metadata.len()));
    if project.join(PROJECT_FILE).is_file() {
        return Ok(project);
    }
    let making = videos.join(format!(".making-{}", std::process::id()));
    let made = (|| {
        let _ = std::fs::remove_dir_all(&making);
        std::fs::create_dir_all(&making)?;
        let file = format!("video.{extension}");
        if std::fs::hard_link(video, making.join(&file)).is_err() {
            std::fs::copy(video, making.join(&file))?;
        }
        let description = serde_json::json!({ "title": stem, "type": "video", "file": file });
        std::fs::write(making.join(PROJECT_FILE), description.to_string())?;
        std::fs::rename(&making, &project)
    })();
    made.map_err(|e| {
        let _ = std::fs::remove_dir_all(&making);
        format!("can't make a project for {} in {}: {e}", video.display(), videos.display())
    })?;
    Ok(project)
}

pub fn forget_videos(folder: &Path, keep: &BTreeSet<PathBuf>) -> Result<(), String> {
    forget(&folder.join(VIDEO_PROJECTS), keep)
}

pub fn stills_of(project: &Path, folder: &Path) -> Option<PathBuf> {
    let metadata = std::fs::metadata(project).ok()?;
    let name = project.file_name().unwrap_or_default().to_string_lossy();
    Some(folder.join(STILLS).join(format!("{name}-{}-{}", metadata.dev(), metadata.ino())))
}

pub fn still(project: &Path, size: (u32, u32), folder: &Path) -> Option<PathBuf> {
    let still = stills_of(project, folder)?.join(still_name(size));
    still.is_file().then_some(still)
}

#[cfg(test)]
pub fn has_stills(project: &Path, folder: &Path) -> bool {
    stills_of(project, folder).is_some_and(|stills| stills.is_dir())
}

pub fn forget_stills(folder: &Path, keep: &[&Path]) -> Result<(), String> {
    let keep = keep.iter().filter_map(|project| stills_of(project, folder)).collect();
    forget(&folder.join(STILLS), &keep)
}

fn still_name((width, height): (u32, u32)) -> String {
    format!("{width}x{height}.png")
}

#[derive(Debug, PartialEq, Eq)]
pub enum Unsaved {
    NotYet,
    Never(String),
}

pub fn save_stills(
    frame: &[u8],
    screens: &BTreeMap<String, PathBuf>,
    sizes: &BTreeMap<String, (u32, u32)>,
    folder: &Path,
    stopped: &dyn Fn() -> bool,
) -> Result<(), Unsaved> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(frame)).read_info().map_err(|_| Unsaved::NotYet)?;
    let mut pixels = vec![0; reader.output_buffer_size().ok_or(Unsaved::Never("it's too big".into()))?];
    let info = reader.next_frame(&mut pixels).map_err(|_| Unsaved::NotYet)?;
    let channels = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgb, png::BitDepth::Eight) => 3,
        (png::ColorType::Rgba, png::BitDepth::Eight) => 4,
        other => return Err(Unsaved::Never(format!("it's {other:?}, not 8-bit RGB"))),
    };
    let mut layout = Vec::new();
    for output in screens.keys() {
        let size = match sizes.get(output) {
            Some(size) => *size,
            None if screens.len() == 1 => (info.width, info.height),
            None => return Err(Unsaved::Never(format!("{output}'s size isn't known"))),
        };
        layout.push((output, size));
    }
    let width: u32 = layout.iter().map(|(_, (width, _))| width).sum();
    let height = layout.iter().map(|(_, (_, height))| *height).max().unwrap_or(0);
    if (info.width, info.height) != (width, height) {
        let why = format!("it's {}x{}, not {width}x{height} like the outputs", info.width, info.height);
        return Err(Unsaved::Never(why));
    }
    let mut left = 0;
    for (output, (width, height)) in layout {
        let row = |y: u32| {
            let start = y as usize * info.line_size + left as usize * channels;
            &pixels[start..start + width as usize * channels]
        };
        let still: Vec<u8> = (0..height).flat_map(row).copied().collect();
        left += width;
        if stopped() {
            return Err(Unsaved::Never("novad is stopping".into()));
        }
        let stills = stills_of(&screens[output], folder)
            .ok_or_else(|| Unsaved::Never(format!("can't read {}", screens[output].display())))?;
        save_png(&still, (width, height), channels == 4, &stills.join(still_name((width, height))))
            .map_err(Unsaved::Never)?;
    }
    Ok(())
}

fn save_png(pixels: &[u8], (width, height): (u32, u32), alpha: bool, path: &Path) -> Result<(), String> {
    let making = path.with_extension(format!("making-{}", std::process::id()));
    let made = (|| -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&making)?), width, height);
        encoder.set_color(if alpha { png::ColorType::Rgba } else { png::ColorType::Rgb });
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        encoder.write_header()?.write_image_data(pixels)?;
        Ok(std::fs::rename(&making, path)?)
    })();
    made.map_err(|e| {
        let _ = std::fs::remove_file(&making);
        format!("can't save {}: {e}", path.display())
    })
}

fn forget(parent: &Path, keep: &BTreeSet<PathBuf>) -> Result<(), String> {
    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("can't read {}: {e}", parent.display())),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !keep.contains(&path) {
            std::fs::remove_dir_all(&path).map_err(|e| format!("can't remove {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    #[test]
    fn the_command_shows_each_output_its_project_on_the_background_layer() {
        let screens = [("DP-1".to_owned(), PathBuf::from("/w/tide")), ("DP-2".to_owned(), PathBuf::from("/w/rain"))];
        let command = command(&screens.into(), Path::new("/assets"), Path::new("/state/frame.png"), "/bin");
        let args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        let line = args.join(" ");
        assert!(line.contains("--layer background") && line.contains("--fullscreen-pause-only-active"), "{line}");
        assert!(line.contains("--silent") && line.contains("--assets-dir /assets"), "{line}");
        assert!(line.ends_with("--screen-root DP-1 --bg /w/tide --screen-root DP-2 --bg /w/rain"), "{line}");
        assert!(line.find("--scaling").unwrap() < line.find("--screen-root").unwrap());
        assert!(line.contains("--screenshot-delay 5") && line.contains("--screenshot /state/frame.png"), "{line}");
    }

    fn frame(width: u32, height: u32) -> Vec<u8> {
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgb);
        let pixels: Vec<u8> = (0..height).flat_map(|y| (0..width).flat_map(move |x| [x as u8, y as u8, 7])).collect();
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
        png
    }

    fn pixels(path: &Path) -> (u32, u32, Vec<u8>) {
        let mut reader =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap())).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        (info.width, info.height, pixels)
    }

    #[test]
    fn a_frame_is_cut_into_a_still_per_output_until_its_project_isnt_used() {
        let dir = TempDir::new("engine-stills");
        let (tide, rain) = (dir.join("tide"), dir.join("rain"));
        std::fs::create_dir_all(&tide).unwrap();
        std::fs::create_dir_all(&rain).unwrap();
        let folder = dir.join("nova");
        let screens = [("DP-1".to_owned(), tide.clone()), ("HDMI-A-1".to_owned(), rain.clone())].into();
        let sizes = [("DP-1".to_owned(), (3, 2)), ("HDMI-A-1".to_owned(), (2, 1))].into();
        assert!(!has_stills(&tide, &folder));
        save_stills(&frame(5, 2), &screens, &sizes, &folder, &|| false).unwrap();
        assert!(has_stills(&tide, &folder));
        assert_eq!(
            pixels(&still(&tide, (3, 2), &folder).unwrap()),
            (3, 2, vec![0, 0, 7, 1, 0, 7, 2, 0, 7, 0, 1, 7, 1, 1, 7, 2, 1, 7])
        );
        assert_eq!(pixels(&still(&rain, (2, 1), &folder).unwrap()), (2, 1, vec![3, 0, 7, 4, 0, 7]));
        assert_eq!(still(&tide, (6, 4), &folder), None, "only for its size");
        forget_stills(&folder, &[&rain]).unwrap();
        assert!(!has_stills(&tide, &folder) && has_stills(&rain, &folder));
    }

    #[test]
    fn a_frame_that_isnt_laid_out_as_the_outputs_is_left() {
        let dir = TempDir::new("engine-odd-frame");
        let tide = dir.join("tide");
        std::fs::create_dir_all(&tide).unwrap();
        let folder = dir.join("nova");
        let screens: BTreeMap<String, PathBuf> =
            [("DP-1".to_owned(), tide.clone()), ("DP-2".to_owned(), tide.clone())].into();
        let sizes = [("DP-1".to_owned(), (3, 2)), ("DP-2".to_owned(), (3, 2))].into();
        let png = frame(5, 2);
        assert!(
            matches!(save_stills(&png, &screens, &sizes, &folder, &|| false), Err(Unsaved::Never(why)) if why.contains("5x2"))
        );
        assert_eq!(
            save_stills(&png[..png.len() - 20], &screens, &sizes, &folder, &|| false),
            Err(Unsaved::NotYet),
            "cut off"
        );
        let no_sizes = save_stills(&png, &screens, &BTreeMap::new(), &folder, &|| false);
        assert!(matches!(no_sizes, Err(Unsaved::Never(why)) if why.contains("DP-1's size")));
        assert!(!has_stills(&tide, &folder));
        save_stills(&png, &[("DP-1".to_owned(), tide.clone())].into(), &BTreeMap::new(), &folder, &|| false).unwrap();
        assert!(still(&tide, (5, 2), &folder).is_some());
    }

    #[test]
    fn steam_assets_are_used_when_installed() {
        let dir = TempDir::new("engine-assets");
        let steam = dir.join("steam");
        assert_eq!(assets(std::slice::from_ref(&steam), &dir.join("nova")), Ok(dir.join("nova").join(EMPTY_ASSETS)));
        assert!(dir.join("nova").join(EMPTY_ASSETS).is_dir());
        std::fs::create_dir_all(steam.join(STEAM_ASSETS)).unwrap();
        assert_eq!(assets(std::slice::from_ref(&steam), &dir.join("nova")), Ok(steam.join(STEAM_ASSETS)));
    }

    #[test]
    fn a_video_gets_a_project_of_its_own_until_it_isnt_used() {
        let dir = TempDir::new("engine-videos");
        let video = dir.join("rain.webm");
        std::fs::write(&video, "frames").unwrap();
        let folder = dir.join("nova");
        let project = video_project(&video, &folder).unwrap();
        let description: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(project.join(PROJECT_FILE)).unwrap()).unwrap();
        assert_eq!(description["type"], "video");
        assert_eq!(std::fs::read_to_string(project.join(description["file"].as_str().unwrap())).unwrap(), "frames");
        assert!(!project.join("video.webm").is_symlink(), "linux-wallpaperengine doesn't follow symlinks");
        assert_eq!(video_project(&video, &folder).unwrap(), project, "made once");
        std::fs::write(&video, "more frames").unwrap();
        let changed = video_project(&video, &folder).unwrap();
        assert_ne!(changed, project);
        forget_videos(&folder, &[changed.clone()].into()).unwrap();
        assert!(!project.exists() && changed.is_dir());
        assert!(video_project(&dir.join("missing.mp4"), &folder).unwrap_err().contains("can't read"));
        std::os::unix::fs::symlink(&video, dir.join("link.webm")).unwrap();
        let linked = video_project(&dir.join("link.webm"), &folder).unwrap();
        assert!(!linked.join("video.webm").is_symlink());
        assert_eq!(linked, changed, "the same file, the same project");
    }
}
