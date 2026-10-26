//! Screenshots (feature `screenshot`): a region the user picks, or the whole
//! focused screen, saved to the screenshot folder and copied to the
//! clipboard.
//!
//! A region shot freezes every screen first: `grim` saves what each output
//! shows into novad's private runtime folder, the UI draws those frames under
//! its region picker (`capture` event), and the picked rectangle is cut out
//! of the frame it was picked on (`save_capture`). So what's saved is what
//! the user saw when they pressed the key, menus and video included, and
//! never the picker itself. Asking for a region while the picker shows
//! closes it, so the same key opens and closes it.
//!
//! Saved shots get an id for the screenshot card's buttons (open, show the
//! folder, delete); novad remembers the last [`KEEP_SAVED`], and only acts
//! on those, so a client can't point it at other files.

use crate::hub::Hub;
use crate::logger::Logger;
use crate::{child, lock};
use nova_protocol::{CaptureFrame, Event, PixelRect, SavedScreenshot, ScreenshotAction};
use std::collections::VecDeque;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The feature's id in `features/`.
pub const FEATURE_ID: &str = "screenshot";
const LOG_SOURCE: &str = "screenshot";

/// Captures outputs (wlr-screencopy).
const GRIM: &str = "grim";
/// Puts a shot on the clipboard.
const WL_COPY: &str = "wl-copy";
/// How long grim or wl-copy may take: under the `nova` CLI's wait for an
/// answer, so a stuck tool's error reaches the keybind's log.
const TOOL_TIMEOUT: Duration = Duration::from_secs(3);
/// Saved shots the card's buttons can still act on.
const KEEP_SAVED: usize = 32;
/// The frozen frames' folder inside novad's runtime folder, readable by the
/// user alone.
const FRAMES_DIR: &str = "capture";
const FRAMES_DIR_MODE: u32 = 0o700;
const PNG_TYPE: &str = "image/png";
/// grim writes frames uncompressed: PNG compression of a 4K screen takes
/// longer than capturing it, and frames are only read back once. The UI
/// reads them too (Qt reads PPM).
const FRAME_TYPE: [&str; 2] = ["-t", "ppm"];
const FRAME_EXTENSION: &str = "ppm";
/// A saved shot's name, from the local time it was taken.
const NAME_PREFIX: &str = "Screenshot ";
const NAME_TIME_FORMAT: &str = "%Y-%m-%d %H-%M-%S";
const EXTENSION: &str = "png";

pub struct Screenshots {
    /// Where frozen frames go.
    frames_dir: PathBuf,
    /// Where the tools are looked for.
    path_var: String,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    /// Held while a shot is taken, so two never interleave.
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// The frozen frames while the region picker shows.
    frames: Option<Vec<CaptureFrame>>,
    /// The latest saved shots, oldest first.
    saved: VecDeque<SavedScreenshot>,
    next_id: u64,
}

impl Screenshots {
    /// `runtime_dir` is novad's own (its socket's folder).
    pub fn new(runtime_dir: &Path, path_var: String, hub: Arc<Hub>, logger: Arc<Logger>) -> Self {
        Self { frames_dir: runtime_dir.join(FRAMES_DIR), path_var, hub, logger, state: Mutex::default() }
    }

    /// Freezes `outputs` and opens the region picker on them, or closes the
    /// picker if it's open. Fails when no UI is connected to show it.
    pub fn pick_region(&self, outputs: &[String]) -> Result<(), String> {
        let mut state = lock(&self.state);
        if state.frames.is_some() {
            self.end_capture(&mut state);
            return Ok(());
        }
        if outputs.is_empty() {
            return Err("there's no screen to capture".into());
        }
        let frames = self.freeze(outputs)?;
        if self.hub.publish(&Event::Capture { frames: frames.clone() }) == 0 {
            remove_frames(&frames);
            return Err("the UI isn't connected to novad, so there's nothing to pick a region with".into());
        }
        state.frames = Some(frames);
        Ok(())
    }

    /// Saves the whole of `output`, or of every screen at once when novad
    /// doesn't know which one has focus.
    pub fn take_screen(&self, output: Option<&str>, folder: &Path) -> Result<(), String> {
        let mut state = lock(&self.state);
        let frame = self.frames_dir.join(format!("screen.{FRAME_EXTENSION}"));
        self.make_frames_dir()?;
        let mut args: Vec<String> = FRAME_TYPE.map(String::from).to_vec();
        if let Some(output) = output {
            args.extend(["-o".to_string(), checked_output(output)?.to_string()]);
        }
        args.push(frame.display().to_string());
        let captured = self.run(GRIM, &args, None).and_then(|()| read(&frame));
        let _ = std::fs::remove_file(&frame);
        let image = Image::from_ppm(&captured?)?;
        self.keep(&mut state, &image, folder)
    }

    /// Saves `rect` of `output`'s frozen frame and closes the picker. A
    /// rectangle that misses the frame, or a failed save, leaves the picker
    /// open.
    pub fn save_region(&self, output: &str, rect: PixelRect, folder: &Path) -> Result<(), String> {
        let mut state = lock(&self.state);
        let frames = state.frames.as_ref().ok_or("no region is being picked")?;
        let frame =
            frames.iter().find(|frame| frame.output == output).ok_or_else(|| format!("{output:?} wasn't captured"))?;
        let image = Image::from_ppm(&read(Path::new(&frame.path))?)?.crop(rect)?;
        // Saved before the frames go, so a failed save (a full disk) leaves
        // the picker to try again.
        let path = write(&image, folder)?;
        self.end_capture(&mut state);
        self.announce(&mut state, &path, &image)
    }

    /// Closes the picker without a shot.
    pub fn cancel(&self) {
        let mut state = lock(&self.state);
        if state.frames.is_some() {
            self.end_capture(&mut state);
        }
    }

    /// Sends `client` the open picker's frames, if it's open.
    pub fn send_state(&self, hub: &Hub, client: crate::hub::ClientId) {
        if let Some(frames) = &lock(&self.state).frames {
            hub.send_to(client, &Event::Capture { frames: frames.clone() });
        }
    }

    /// Opens a saved shot, its folder, or deletes it.
    pub fn act(&self, id: u64, action: ScreenshotAction) -> Result<(), String> {
        let mut state = lock(&self.state);
        let index = state
            .saved
            .iter()
            .position(|shot| shot.id == id)
            .ok_or_else(|| format!("no screenshot {id} (it's from before novad started, or deleted)"))?;
        let path = PathBuf::from(&state.saved[index].path);
        act_on(&path, action, &self.path_var, &self.logger, LOG_SOURCE)?;
        if action == ScreenshotAction::Delete {
            state.saved.remove(index);
        }
        Ok(())
    }

    /// Captures each output into the frames folder, all at once.
    fn freeze(&self, outputs: &[String]) -> Result<Vec<CaptureFrame>, String> {
        self.make_frames_dir()?;
        let frames: Vec<CaptureFrame> = outputs
            .iter()
            .map(|output| {
                let path = self.frames_dir.join(format!("{}.{FRAME_EXTENSION}", checked_output(output)?));
                Ok(CaptureFrame { output: output.clone(), path: path.display().to_string() })
            })
            .collect::<Result<_, String>>()?;
        let results: Vec<Result<(), String>> = std::thread::scope(|scope| {
            let running: Vec<_> = frames
                .iter()
                .map(|frame| {
                    let args = [
                        FRAME_TYPE[0].into(),
                        FRAME_TYPE[1].into(),
                        "-o".into(),
                        frame.output.clone(),
                        frame.path.clone(),
                    ];
                    scope.spawn(move || self.run(GRIM, &args, None))
                })
                .collect();
            running
                .into_iter()
                .map(|thread| thread.join().unwrap_or_else(|_| Err("grim's thread panicked".into())))
                .collect()
        });
        if let Some(Err(e)) = results.into_iter().find(Result::is_err) {
            remove_frames(&frames);
            return Err(e);
        }
        Ok(frames)
    }

    /// Writes a shot into `folder`, copies it and tells the UI.
    fn keep(&self, state: &mut State, image: &Image, folder: &Path) -> Result<(), String> {
        let path = write(image, folder)?;
        self.announce(state, &path, image)
    }

    /// Copies a saved shot and tells the UI, which says whether it's on the
    /// clipboard: the file is kept even when the clipboard isn't there.
    fn announce(&self, state: &mut State, path: &Path, image: &Image) -> Result<(), String> {
        let (width, height) = (image.width, image.height);
        self.logger.info(LOG_SOURCE, &format!("saved {} ({width}x{height})", path.display()));
        let copied = std::fs::File::open(path)
            .map_err(|e| format!("can't read {} back: {e}", path.display()))
            .and_then(|file| self.run(WL_COPY, &["--type".into(), PNG_TYPE.into()], Some(file)));
        if let Err(e) = &copied {
            self.logger.warn(LOG_SOURCE, &format!("saved, but couldn't copy it: {e}"));
        }
        state.next_id += 1;
        let shot = SavedScreenshot {
            id: state.next_id,
            path: path.display().to_string(),
            width,
            height,
            copied: copied.is_ok(),
        };
        if state.saved.len() == KEEP_SAVED {
            state.saved.pop_front();
        }
        state.saved.push_back(shot.clone());
        self.hub.publish(&Event::ScreenshotSaved(shot));
        Ok(())
    }

    fn end_capture(&self, state: &mut State) {
        if let Some(frames) = state.frames.take() {
            remove_frames(&frames);
        }
        self.hub.publish(&Event::CaptureEnded);
    }

    fn make_frames_dir(&self) -> Result<(), String> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(FRAMES_DIR_MODE)
            .create(&self.frames_dir)
            .map_err(|e| format!("can't make {}: {e}", self.frames_dir.display()))
    }

    /// Runs `program` with `args` (and `stdin`) and waits up to
    /// [`TOOL_TIMEOUT`] for it to succeed. Its error output becomes the
    /// error, except wl-copy's: it leaves a copy of itself running to serve
    /// the clipboard, which would hold the pipe open.
    fn run(&self, program: &str, args: &[String], stdin: Option<std::fs::File>) -> Result<(), String> {
        let errors_readable = program != WL_COPY;
        let mut child = Command::new(program)
            .args(args)
            .env("PATH", &self.path_var)
            .stdin(stdin.map_or_else(Stdio::null, Stdio::from))
            .stdout(Stdio::null())
            .stderr(if errors_readable { Stdio::piped() } else { Stdio::null() })
            .spawn()
            .map_err(|e| format!("can't run {program}: {e}"))?;
        let errors = child::drain(child.stderr.take());
        let status = match child::wait_within(&mut child, TOOL_TIMEOUT) {
            Ok(Some(status)) => status,
            Ok(None) => {
                child::kill_and_reap(&mut child);
                return Err(format!("{program} didn't finish within {} s", TOOL_TIMEOUT.as_secs()));
            }
            Err(e) => {
                child::kill_and_reap(&mut child);
                return Err(format!("{program} couldn't be waited on: {e}"));
            }
        };
        let message = errors.text();
        match status.success() {
            true => Ok(()),
            false => Err(format!("{program} failed ({status}): {}", message.trim())),
        }
    }
}

/// Opens a saved file (`path`), its folder, or deletes it; a file that's
/// already gone counts as deleted. Recordings use it too.
pub fn act_on(
    path: &Path,
    action: ScreenshotAction,
    path_var: &str,
    logger: &Arc<Logger>,
    log_source: &str,
) -> Result<(), String> {
    match action {
        ScreenshotAction::Open => crate::launcher::open(path, path_var, logger),
        ScreenshotAction::Folder => crate::launcher::open(path.parent().unwrap_or(Path::new("/")), path_var, logger),
        ScreenshotAction::Delete => {
            match std::fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(format!("can't delete {}: {e}", path.display()));
                }
                _ => {}
            }
            logger.info(log_source, &format!("deleted {}", path.display()));
            Ok(())
        }
    }
}

/// An output name that's safe as a file name: it comes from the compositor.
fn checked_output(output: &str) -> Result<&str, String> {
    let safe = !output.is_empty() && !output.starts_with('.') && !output.contains(['/', '\0']);
    safe.then_some(output).ok_or_else(|| format!("odd output name {output:?}"))
}

fn remove_frames(frames: &[CaptureFrame]) {
    for frame in frames {
        let _ = std::fs::remove_file(&frame.path);
    }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))
}

/// A frame or a shot: 8-bit RGB pixels, row after row.
struct Image {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

/// Bytes per RGB pixel.
const RGB: usize = 3;
/// The only PPM grim writes: binary RGB, 8 bits per channel.
const PPM_MAGIC: &str = "P6";
const PPM_MAX_VALUE: &str = "255";

impl Image {
    /// Reads a binary PPM: `P6`, width, height and 255, each followed by
    /// one whitespace byte, then the pixels.
    fn from_ppm(ppm: &[u8]) -> Result<Self, String> {
        let odd = || "odd frame: not an 8-bit binary PPM".to_string();
        let mut fields = Vec::new();
        let mut at = 0;
        while fields.len() < 4 {
            while ppm.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            let start = at;
            while ppm.get(at).is_some_and(|byte| !byte.is_ascii_whitespace()) {
                at += 1;
            }
            let field = ppm.get(start..at).filter(|field| !field.is_empty()).ok_or_else(odd)?;
            fields.push(std::str::from_utf8(field).map_err(|_| odd())?);
            // The one whitespace byte after the field.
            at += 1;
        }
        let parse = |field: &str| field.parse::<u32>().map_err(|_| odd());
        if fields[0] != PPM_MAGIC || fields[3] != PPM_MAX_VALUE {
            return Err(odd());
        }
        let (width, height) = (parse(fields[1])?, parse(fields[2])?);
        let size = width as usize * height as usize * RGB;
        let rgb = ppm.get(at..at + size).ok_or_else(|| format!("the {width}x{height} frame is cut short"))?;
        Ok(Self { width, height, rgb: rgb.to_vec() })
    }

    /// `rect` of the image. The part outside it is dropped; nothing left is
    /// an error.
    fn crop(&self, rect: PixelRect) -> Result<Self, String> {
        let left = rect.x.min(self.width);
        let top = rect.y.min(self.height);
        let width = rect.width.min(self.width - left);
        let height = rect.height.min(self.height - top);
        if width == 0 || height == 0 {
            return Err(format!("the region {rect:?} isn't on the {}x{} frame", self.width, self.height));
        }
        let row_bytes = width as usize * RGB;
        let mut rgb = Vec::with_capacity(row_bytes * height as usize);
        for row in top..top + height {
            let start = (row as usize * self.width as usize + left as usize) * RGB;
            rgb.extend_from_slice(&self.rgb[start..start + row_bytes]);
        }
        Ok(Self { width, height, rgb })
    }

    /// The image as a PNG, compressed fast: a 4K shot is saved in a blink,
    /// a little bigger than at the default level.
    fn to_png(&self) -> Result<Vec<u8>, String> {
        let failed = |e: png::EncodingError| format!("can't encode the shot: {e}");
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(failed)?;
        writer.write_image_data(&self.rgb).map_err(failed)?;
        writer.finish().map_err(failed)?;
        Ok(out)
    }
}

/// Encodes `image` and writes it into `folder` (see [`save`]).
fn write(image: &Image, folder: &Path) -> Result<PathBuf, String> {
    save(&image.to_png()?, folder)
}

/// Writes `png` into `folder` under a name from the local time, never over
/// an existing file: through a temporary file that's moved into place, so
/// the name only ever shows a whole shot.
fn save(png: &[u8], folder: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("can't make {}: {e}", folder.display()))?;
    let stamp = time_stamp();
    let temporary = folder.join(format!(".{NAME_PREFIX}{stamp}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, png).map_err(|e| format!("can't write {}: {e}", temporary.display()))?;
    let result = place(&temporary, &folder.join(format!("{NAME_PREFIX}{stamp}")), EXTENSION);
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// The local time, for a saved file's name.
pub fn time_stamp() -> String {
    jiff::Zoned::now().strftime(NAME_TIME_FORMAT).to_string()
}

/// Moves the finished `temporary` file to `<stem>.<extension>`, or
/// `<stem> (2).<extension>` and so on when that's taken: never over an
/// existing file. Recordings use it too.
pub fn place(temporary: &Path, stem: &Path, extension: &str) -> Result<PathBuf, String> {
    (1..)
        .map(|n| match n {
            1 => PathBuf::from(format!("{}.{extension}", stem.display())),
            n => PathBuf::from(format!("{} ({n}).{extension}", stem.display())),
        })
        .find_map(|path| match rename_new(temporary, &path) {
            Ok(()) => Some(Ok(path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
            Err(e) => Some(Err(format!("can't save {}: {e}", path.display()))),
        })
        .unwrap_or_else(|| Err("no free file name".into()))
}

/// Renames `from` to `to` unless `to` exists (`RENAME_NOREPLACE`, which
/// filesystems without hard links, like exFAT, have too).
fn rename_new(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c = |path: &Path| std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(std::io::Error::other);
    let (from, to) = (c(from)?, c(to)?);
    // SAFETY: both are NUL-terminated paths that live for the whole call.
    let renamed =
        unsafe { libc::renameat2(libc::AT_FDCWD, from.as_ptr(), libc::AT_FDCWD, to.as_ptr(), libc::RENAME_NOREPLACE) };
    match renamed {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{ClientQueue, Outgoing};
    use nova_protocol::{LogLevel, Topic};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc::Receiver;
    use std::time::Instant;

    /// The fake screens' size: grim's frames are the outputs' pixels.
    const FRAME_WIDTH: u32 = 8;
    const FRAME_HEIGHT: u32 = 6;

    /// Screenshots with fake grim (a frame per output, each pixel naming its
    /// output, column and row) and wl-copy (keeps what it was given), and a
    /// UI subscribed to the screenshot topic.
    struct Fixture {
        dir: PathBuf,
        screenshots: Screenshots,
        hub: Arc<Hub>,
        ui: Receiver<Outgoing>,
        _ui_stream: UnixStream,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("novad-screenshot-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let bin = dir.join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            for output in ["DP-1", "HDMI-A-1"] {
                std::fs::write(dir.join(format!("{output}.ppm")), frame(output)).unwrap();
            }
            // grim -t ppm [-o OUTPUT] FILE; the whole desktop is DP-1's frame.
            fake(
                &bin,
                GRIM,
                &format!(
                    "[ \"$1 $2\" = \"-t ppm\" ] || {{ echo \"not asked for ppm\" >&2; exit 1; }}; shift 2\n\
                     output=DP-1\nif [ \"$1\" = -o ]; then output=$2; shift 2; fi\n\
                     [ -f {dir}/$output.ppm ] || {{ echo \"unknown output $output\" >&2; exit 1; }}\n\
                     cp {dir}/$output.ppm \"$1\"",
                    dir = dir.display()
                ),
            );
            fake(&bin, WL_COPY, &format!("cat > {}/clipboard", dir.display()));
            let hub = Arc::new(Hub::default());
            let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
            let (ui_stream, other_end) = UnixStream::pair().unwrap();
            let (queue, ui) = ClientQueue::new(&other_end).unwrap();
            let id = hub.register(queue);
            hub.subscribe(id, Topic::Screenshot);
            // The fakes first; the system's for the tools they use.
            let path_var = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
            let screenshots = Screenshots::new(&dir.join("runtime"), path_var, Arc::clone(&hub), logger);
            Self { dir, screenshots, hub, ui, _ui_stream: ui_stream }
        }

        fn folder(&self) -> PathBuf {
            self.dir.join("Pictures/Screenshots")
        }

        /// The events the UI got since last asked.
        fn events(&self) -> Vec<serde_json::Value> {
            self.ui
                .try_iter()
                .filter_map(|message| match message {
                    Outgoing::Line(line) => serde_json::from_str(&line).ok(),
                    Outgoing::Flushed(_) => None,
                })
                .collect()
        }

        fn saved(&self) -> Vec<PathBuf> {
            let mut saved: Vec<PathBuf> =
                std::fs::read_dir(self.folder()).unwrap().map(|entry| entry.unwrap().path()).collect();
            saved.sort();
            saved
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn fake(bin: &Path, name: &str, body: &str) {
        let path = bin.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A frame as grim writes it, whose pixel (x, y) is (output's first
    /// byte, x, y).
    fn frame(output: &str) -> Vec<u8> {
        let mut ppm = format!("P6\n{FRAME_WIDTH} {FRAME_HEIGHT}\n255\n").into_bytes();
        for y in 0..FRAME_HEIGHT {
            for x in 0..FRAME_WIDTH {
                ppm.extend([output.as_bytes()[0], x as u8, y as u8]);
            }
        }
        ppm
    }

    /// A PNG's width, height and RGB pixels.
    fn decode(png: &[u8]) -> (u32, u32, Vec<[u8; 3]>) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        let pixels = pixels[..info.buffer_size()].chunks(3).map(|pixel| pixel.try_into().unwrap()).collect();
        (info.width, info.height, pixels)
    }

    fn outputs() -> Vec<String> {
        vec!["DP-1".into(), "HDMI-A-1".into()]
    }

    #[test]
    fn a_picked_region_is_cut_from_the_frozen_frame_saved_and_copied() {
        let fixture = Fixture::new("region");
        fixture.screenshots.pick_region(&outputs()).unwrap();
        let events = fixture.events();
        assert_eq!(events.len(), 1, "{events:?}");
        let frames = &events[0]["data"]["frames"];
        assert_eq!(frames.as_array().unwrap().len(), 2, "{events:?}");
        let runtime = fixture.dir.join("runtime").join(FRAMES_DIR);
        assert_eq!(std::fs::metadata(&runtime).unwrap().permissions().mode() & 0o777, FRAMES_DIR_MODE);

        let rect = PixelRect { x: 2, y: 1, width: 3, height: 2 };
        fixture.screenshots.save_region("HDMI-A-1", rect, &fixture.folder()).unwrap();
        let saved = fixture.saved();
        assert_eq!(saved.len(), 1, "{saved:?}");
        let png = std::fs::read(&saved[0]).unwrap();
        let (width, height, pixels) = decode(&png);
        assert_eq!((width, height), (3, 2));
        assert_eq!(pixels[0], [b'H', 2, 1]);
        assert_eq!(pixels[5], [b'H', 4, 2]);
        assert_eq!(std::fs::read(fixture.dir.join("clipboard")).unwrap(), png, "copied as saved");
        assert!(std::fs::read_dir(&runtime).unwrap().next().is_none(), "the frames are removed");

        let events = fixture.events();
        assert_eq!(events[0]["event"], "capture_ended", "{events:?}");
        assert_eq!(events[1]["data"]["path"], saved[0].display().to_string());
        assert_eq!(events[1]["data"]["width"], 3);
    }

    #[test]
    fn a_region_partly_off_the_frame_is_trimmed_and_one_wholly_off_keeps_the_picker() {
        let fixture = Fixture::new("off-frame");
        fixture.screenshots.pick_region(&outputs()).unwrap();
        let off = PixelRect { x: FRAME_WIDTH, y: 0, width: 4, height: 4 };
        assert!(fixture.screenshots.save_region("DP-1", off, &fixture.folder()).unwrap_err().contains("isn't on"));
        assert!(fixture.screenshots.save_region("eDP-9", off, &fixture.folder()).is_err());

        let partly = PixelRect { x: FRAME_WIDTH - 2, y: FRAME_HEIGHT - 1, width: 10, height: 10 };
        fixture.screenshots.save_region("DP-1", partly, &fixture.folder()).unwrap();
        let (width, height, _) = decode(&std::fs::read(&fixture.saved()[0]).unwrap());
        assert_eq!((width, height), (2, 1));
    }

    #[test]
    fn asking_again_or_cancelling_closes_the_picker_without_a_shot() {
        let fixture = Fixture::new("toggle");
        fixture.screenshots.pick_region(&outputs()).unwrap();
        fixture.screenshots.pick_region(&outputs()).unwrap();
        let events = fixture.events();
        assert_eq!(events.last().unwrap()["event"], "capture_ended", "{events:?}");
        let rect = PixelRect { x: 0, y: 0, width: 1, height: 1 };
        assert!(fixture.screenshots.save_region("DP-1", rect, &fixture.folder()).is_err(), "nothing is picked");

        fixture.screenshots.pick_region(&outputs()).unwrap();
        fixture.screenshots.cancel();
        fixture.screenshots.cancel();
        let ended = fixture.events().into_iter().filter(|event| event["event"] == "capture_ended").count();
        assert_eq!(ended, 1, "a second cancel has nothing to close");
        assert!(!fixture.folder().exists(), "no shot was saved");
    }

    #[test]
    fn the_picker_needs_a_ui_and_working_grim() {
        let fixture = Fixture::new("failing");
        let broken = vec!["DP-1".to_string(), "DP-7".to_string()];
        assert!(fixture.screenshots.pick_region(&broken).unwrap_err().contains("unknown output DP-7"));
        let runtime = fixture.dir.join("runtime").join(FRAMES_DIR);
        assert!(std::fs::read_dir(&runtime).unwrap().next().is_none(), "the other frame is removed");
        assert!(fixture.screenshots.pick_region(&["../x".to_string()]).is_err(), "odd output names");

        let lonely = Screenshots::new(
            &fixture.dir.join("lonely"),
            fixture.screenshots.path_var.clone(),
            Arc::new(Hub::default()),
            Arc::clone(&fixture.screenshots.logger),
        );
        assert!(lonely.pick_region(&outputs()).unwrap_err().contains("isn't connected"));
        assert!(lonely.pick_region(&outputs()).is_err(), "and it didn't stay open");
        let _ = &fixture.hub;
    }

    #[test]
    fn a_screen_shot_saves_the_focused_output_without_overwriting() {
        let fixture = Fixture::new("screen");
        fixture.screenshots.take_screen(Some("HDMI-A-1"), &fixture.folder()).unwrap();
        fixture.screenshots.take_screen(None, &fixture.folder()).unwrap();
        let saved = fixture.saved();
        assert_eq!(saved.len(), 2, "two in one second get different names: {saved:?}");
        let first_pixels: Vec<u8> = saved.iter().map(|path| decode(&std::fs::read(path).unwrap()).2[0][0]).collect();
        assert!(first_pixels.contains(&b'H') && first_pixels.contains(&b'D'), "{first_pixels:?}");
        assert!(
            saved.iter().all(|path| !path.file_name().unwrap().to_str().unwrap().starts_with('.')),
            "no temp files"
        );
    }

    #[test]
    fn card_actions_only_reach_shots_novad_saved() {
        let fixture = Fixture::new("actions");
        fixture.screenshots.take_screen(Some("DP-1"), &fixture.folder()).unwrap();
        let id = fixture.events()[0]["data"]["id"].as_u64().unwrap();
        assert!(fixture.screenshots.act(id + 1, ScreenshotAction::Delete).is_err());
        fixture.screenshots.act(id, ScreenshotAction::Delete).unwrap();
        assert!(fixture.saved().is_empty());
        assert!(fixture.screenshots.act(id, ScreenshotAction::Open).is_err(), "deleted shots are forgotten");
    }

    #[test]
    fn a_failed_save_keeps_the_picker_and_a_failed_copy_still_saves() {
        let fixture = Fixture::new("failed-save");
        fixture.screenshots.pick_region(&outputs()).unwrap();
        // A file where the folder should be.
        let blocked = fixture.dir.join("blocked");
        std::fs::write(&blocked, "").unwrap();
        let rect = PixelRect { x: 0, y: 0, width: 2, height: 2 };
        assert!(fixture.screenshots.save_region("DP-1", rect, &blocked.join("Screenshots")).is_err());
        fixture.events();
        fixture.screenshots.save_region("DP-1", rect, &fixture.folder()).unwrap();
        let events = fixture.events();
        assert_eq!(events[0]["event"], "capture_ended", "only now: {events:?}");
        assert_eq!(events[1]["data"]["copied"], true);

        // No clipboard: saved all the same, and said so.
        fake(&fixture.dir.join("bin"), WL_COPY, "exit 1");
        fixture.screenshots.take_screen(Some("DP-1"), &fixture.folder()).unwrap();
        let events = fixture.events();
        assert_eq!(events[0]["data"]["copied"], false, "{events:?}");
        assert_eq!(fixture.saved().len(), 2);
    }

    #[test]
    fn ppm_frames_are_read_and_odd_ones_refused() {
        let image = Image::from_ppm(&frame("DP-1")).unwrap();
        assert_eq!((image.width, image.height), (FRAME_WIDTH, FRAME_HEIGHT));
        assert_eq!(&image.rgb[..RGB], &[b'D', 0, 0]);
        let mut short = frame("DP-1");
        short.pop();
        assert!(Image::from_ppm(&short).err().unwrap().contains("cut short"));
        assert!(Image::from_ppm(b"P3\n1 1\n255\n0 0 0").is_err(), "ASCII PPM");
        assert!(Image::from_ppm(b"P6\n1 1\n65535\n\0\0\0\0\0\0").is_err(), "16-bit");
        assert!(Image::from_ppm(b"").is_err());
    }

    #[test]
    fn a_tool_that_hangs_is_stopped() {
        let fixture = Fixture::new("hang");
        let started = Instant::now();
        let error = fixture.screenshots.run("sleep", &["30".into()], None).unwrap_err();
        assert!(error.contains("didn't finish"), "{error}");
        assert!(started.elapsed() < TOOL_TIMEOUT * 2);
    }
}
