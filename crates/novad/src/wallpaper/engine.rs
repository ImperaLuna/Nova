//! Running linux-wallpaperengine (feature `wallpaper-engine`): one process
//! shows every output's dynamic wallpaper, each with `--screen-root <output>
//! --bg <project>`, and is started again whenever that set changes.
//!
//! Flags from the IMP-168 spike on niri: the background layer (its default,
//! bottom, is cloned onto every workspace in niri's overview), pausing only
//! under an active fullscreen window (otherwise any output-sized window,
//! even on a hidden workspace, freezes every screen), no sound, and an
//! assets folder: Wallpaper Engine's own from Steam when it's installed,
//! otherwise an empty one, which simple scenes and videos don't need more
//! than.
//!
//! It only opens projects, and only finds files that are really inside the
//! project's folder (not through a symlink), so a video gets a project of
//! its own in the state folder with a hard link to it, or a copy when it's
//! on another disk or a link isn't allowed.
//!
//! Each run also writes one frame of what it shows (`--screenshot`): every
//! output side by side in one image, in the order of their names, each as
//! wide and as tall as its surface in pixels, from the top. novad cuts it
//! into one still per output (`save_stills`) and keeps them by project and
//! size, so awww can show a sharp picture while the engine starts next time
//! instead of the project's small preview.

use super::source::PROJECT_FILE;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PROGRAM: &str = "linux-wallpaperengine";
const FLAGS: &[&str] = &[
    "--silent",
    "--layer",
    "background",
    "--fullscreen-pause-only-active",
    // Fills the output and crops, like awww's images.
    "--scaling",
    "fill",
    // The frames it waits before writing its frame: its most.
    "--screenshot-delay",
    "5",
];
/// Wallpaper Engine's assets in a Steam library.
const STEAM_ASSETS: &str = "steamapps/common/wallpaper_engine/assets";
/// Nova's folder in the state folder: the empty assets folder and the
/// videos' projects.
const STATE_FOLDER: &str = "wallpaper-engine";
const EMPTY_ASSETS: &str = "assets";
const VIDEO_PROJECTS: &str = "videos";
/// Where a run writes its frame, and where the stills cut from it are kept
/// (`<project>/<width>x<height>.png`).
const FRAME_FILE: &str = "frame.png";
const STILLS: &str = "stills";
/// NixOS keeps the GPU driver's libraries here, outside every package, and
/// nixpkgs' linux-wallpaperengine doesn't look there: without it, NVIDIA
/// video decoding falls back to a copy on the CPU (15% of a core instead of
/// 3.7% in the IMP-168 spike). Remove once nixpkgs wraps the program with
/// the driver path.
const NIXOS_DRIVER_LIBS: &str = "/run/opengl-driver/lib";

/// Where its files go: `<state>/wallpaper-engine`.
pub fn state_folder(state_dir: &Path) -> PathBuf {
    state_dir.join(STATE_FOLDER)
}

/// Where a run writes its frame, in `folder`.
pub fn frame_file(folder: &Path) -> PathBuf {
    folder.join(FRAME_FILE)
}

/// The command that shows `screens` (output name to project folder) and
/// writes its frame to `frame`.
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

/// Wallpaper Engine's assets from the first Steam library that has them,
/// else an empty folder in `folder` (made if missing).
pub fn assets(steam_roots: &[PathBuf], folder: &Path) -> Result<PathBuf, String> {
    if let Some(steam) = steam_roots.iter().map(|root| root.join(STEAM_ASSETS)).find(|assets| assets.is_dir()) {
        return Ok(steam);
    }
    let empty = folder.join(EMPTY_ASSETS);
    std::fs::create_dir_all(&empty).map_err(|e| format!("can't make {}: {e}", empty.display()))?;
    Ok(empty)
}

/// The project that plays `video`, made in `folder` if it isn't there yet.
/// Named after the video's name, file (device and inode), size and time,
/// so another file or a changed one gets its own.
pub fn video_project(video: &Path, folder: &Path) -> Result<PathBuf, String> {
    // A hard link to a symlink would be a symlink, which it won't follow.
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
    // Made under another name and renamed into place, so a half-made one
    // is never used.
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

/// Removes the videos' projects in `folder` that aren't in `keep`.
pub fn forget_videos(folder: &Path, keep: &BTreeSet<PathBuf>) -> Result<(), String> {
    forget(&folder.join(VIDEO_PROJECTS), keep)
}

/// The folder in `folder` with `project`'s stills, named like a video's
/// project after its name and folder (device and inode), so a project put
/// back in another folder or another one with the same name gets its own.
/// `None` when it can't be read.
pub fn stills_of(project: &Path, folder: &Path) -> Option<PathBuf> {
    let metadata = std::fs::metadata(project).ok()?;
    let name = project.file_name().unwrap_or_default().to_string_lossy();
    Some(folder.join(STILLS).join(format!("{name}-{}-{}", metadata.dev(), metadata.ino())))
}

/// `project`'s still for an output `size` pixels big, if one was kept.
pub fn still(project: &Path, size: (u32, u32), folder: &Path) -> Option<PathBuf> {
    let still = stills_of(project, folder)?.join(still_name(size));
    still.is_file().then_some(still)
}

/// Whether any still of `project` was kept.
#[cfg(test)]
pub fn has_stills(project: &Path, folder: &Path) -> bool {
    stills_of(project, folder).is_some_and(|stills| stills.is_dir())
}

/// Removes the stills in `folder` of projects that aren't in `keep`.
pub fn forget_stills(folder: &Path, keep: &[&Path]) -> Result<(), String> {
    let keep = keep.iter().filter_map(|project| stills_of(project, folder)).collect();
    forget(&folder.join(STILLS), &keep)
}

fn still_name((width, height): (u32, u32)) -> String {
    format!("{width}x{height}.png")
}

/// Why [`save_stills`] kept nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum Unsaved {
    /// The frame is cut off: it's still being written.
    NotYet,
    /// It never will from this frame.
    Never(String),
}

/// Cuts `frame`, a run's frame of `screens`, into a still for each output
/// and keeps them in `folder`. `sizes`: each output's size in pixels; with
/// one output, the frame's own size stands in for an unknown one. Nothing
/// is kept from a frame that isn't laid out as those sizes say (with an
/// output at a fractional scale, which the engine draws at a size of its
/// own, say).
pub fn save_stills(
    frame: &[u8],
    screens: &BTreeMap<String, PathBuf>,
    sizes: &BTreeMap<String, (u32, u32)>,
    folder: &Path,
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
        let stills = stills_of(&screens[output], folder)
            .ok_or_else(|| Unsaved::Never(format!("can't read {}", screens[output].display())))?;
        save_png(&still, (width, height), channels == 4, &stills.join(still_name((width, height))))
            .map_err(Unsaved::Never)?;
    }
    Ok(())
}

/// Written under another name and renamed into place, so awww never reads
/// half of one.
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

/// Removes the folders in `parent` that aren't in `keep`.
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
        // --scaling applies to the screens after it.
        assert!(line.find("--scaling").unwrap() < line.find("--screen-root").unwrap());
        assert!(line.contains("--screenshot-delay 5") && line.contains("--screenshot /state/frame.png"), "{line}");
    }

    /// A PNG `width` pixels wide whose pixel at x is (x, y, 7).
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
        // Side by side in name order, the shorter one from the top.
        let sizes = [("DP-1".to_owned(), (3, 2)), ("HDMI-A-1".to_owned(), (2, 1))].into();
        assert!(!has_stills(&tide, &folder));
        save_stills(&frame(5, 2), &screens, &sizes, &folder).unwrap();
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
            matches!(save_stills(&png, &screens, &sizes, &folder), Err(Unsaved::Never(why)) if why.contains("5x2"))
        );
        assert_eq!(save_stills(&png[..png.len() - 20], &screens, &sizes, &folder), Err(Unsaved::NotYet), "cut off");
        let no_sizes = save_stills(&png, &screens, &BTreeMap::new(), &folder);
        assert!(matches!(no_sizes, Err(Unsaved::Never(why)) if why.contains("DP-1's size")));
        assert!(!has_stills(&tide, &folder));
        // One output: the frame is its size.
        save_stills(&png, &[("DP-1".to_owned(), tide.clone())].into(), &BTreeMap::new(), &folder).unwrap();
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
        // A changed video gets a new one; the old one goes once unused.
        std::fs::write(&video, "more frames").unwrap();
        let changed = video_project(&video, &folder).unwrap();
        assert_ne!(changed, project);
        forget_videos(&folder, &[changed.clone()].into()).unwrap();
        assert!(!project.exists() && changed.is_dir());
        assert!(video_project(&dir.join("missing.mp4"), &folder).unwrap_err().contains("can't read"));
        // Through a symlink: the project gets the file itself.
        std::os::unix::fs::symlink(&video, dir.join("link.webm")).unwrap();
        let linked = video_project(&dir.join("link.webm"), &folder).unwrap();
        assert!(!linked.join("video.webm").is_symlink());
        assert_eq!(linked, changed, "the same file, the same project");
    }
}
