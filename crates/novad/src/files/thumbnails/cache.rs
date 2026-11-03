//! The shared thumbnail cache (freedesktop.org Thumbnail Managing Standard
//! 0.9, `$XDG_CACHE_HOME/thumbnails`), read and written the way GNOME and
//! KDE apps do, so a thumbnail any of them made shows in Nova Files and the
//! other way round.
//!
//! A file's thumbnail is named after the MD5 of its URI, as GLib writes
//! URIs ([`uri`]), in the folder of its size (`large/` here, 256 px). It's
//! good while its `Thumb::URI` and `Thumb::MTime` text chunks still name
//! the file and its modification time; anything else is made again. A file
//! that couldn't be made gets an empty PNG with the same chunks under
//! `fail/nova-<version>/`, so it isn't tried again until the file changes.
//! Folders are private (0700) and files are written whole (temp file and
//! rename, 0600), as the standard asks.

use md5::{Digest, Md5};
use nova_protocol::FILES_THUMBNAIL_PIXELS;
use std::io::{BufReader, Cursor};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The size folder for [`FILES_THUMBNAIL_PIXELS`].
const LARGE: &str = "large";
/// Where apps mark files they couldn't make a thumbnail of, each in a
/// folder of its own.
const FAIL: &str = "fail";
const OWN_FAIL: &str = concat!("nova-", env!("CARGO_PKG_VERSION"));
const URI_KEY: &str = "Thumb::URI";
const MTIME_KEY: &str = "Thumb::MTime";
const SIZE_KEY: &str = "Thumb::Size";
const SOFTWARE_KEY: &str = "Software";
const SOFTWARE: &str = concat!("Nova ", env!("CARGO_PKG_VERSION"));
const FOLDER_MODE: u32 = 0o700;
/// Bytes GLib keeps as they are in a `file://` URI's path (its
/// `UNSAFE_PATH` table), beside ASCII letters and digits; every other byte
/// is `%XX`. The MD5 is of these exact bytes, so they must match.
const URI_KEEP: &[u8] = b"!$&'()*+,-./:=@_~";
/// The most memory one thumbnailer's picture may take to read: a 4096 px
/// square in RGBA16, far more than any asked for.
const MAX_DECODE_BYTES: usize = 4096 * 4096 * 8;

/// The file a thumbnail is for, as it is now.
#[derive(Debug, Clone)]
pub struct Original {
    pub path: PathBuf,
    pub uri: String,
    /// Its modification time, in whole seconds since 1970, as the cache
    /// keeps it.
    pub mtime: i64,
    /// The same in milliseconds, as a folder's listing shows it.
    pub modified_ms: Option<i64>,
    pub size: u64,
}

impl Original {
    /// The file at `path` (absolute), as its metadata says.
    pub fn new(path: &Path, metadata: &std::fs::Metadata) -> Self {
        Self {
            path: path.to_owned(),
            uri: uri(path),
            mtime: metadata.mtime(),
            modified_ms: super::super::folder::modified_ms(metadata),
            size: metadata.size(),
        }
    }
}

/// Why no thumbnail was stored.
#[derive(Debug)]
pub enum Failed {
    /// The file, or what its thumbnailer made of it: it's marked failed.
    File(String),
    /// Something here (the cache can't be written): it's tried again.
    Here(String),
}

/// `path` (absolute) as GLib's `g_filename_to_uri` writes it.
pub fn uri(path: &Path) -> String {
    format!("file://{}", crate::notifications::image::percent_encoded(path.as_os_str().as_bytes(), URI_KEEP))
}

/// The cache folder.
pub struct Cache {
    root: PathBuf,
    /// This cache's number in novad: scratch files of one opened earlier
    /// (Nova Files turned off and on) never share a name with its own.
    instance: usize,
}

/// The next [`Cache::instance`].
static NEXT_INSTANCE: AtomicUsize = AtomicUsize::new(0);

impl Cache {
    /// The cache at `root` (`$XDG_CACHE_HOME/thumbnails`), made when it's
    /// first written to.
    pub fn new(root: PathBuf) -> Self {
        Self { root, instance: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed) }
    }

    /// Whether `path` is in the cache, as named or through links:
    /// thumbnails of thumbnails aren't made.
    pub fn holds(&self, path: &Path) -> bool {
        let real = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        path.starts_with(&self.root) || real(path).starts_with(real(&self.root))
    }

    /// The thumbnail of `original` that's still good, if there is one.
    pub fn found(&self, original: &Original) -> Option<PathBuf> {
        let path = self.thumbnail_path(original);
        describes(&path, original).then_some(path)
    }

    /// Whether making a thumbnail of `original`, as it is now, failed
    /// before.
    pub fn failed(&self, original: &Original) -> bool {
        describes(&self.fail_path(original), original)
    }

    /// Stores the picture a thumbnailer wrote at `made` (a PNG) as
    /// `original`'s thumbnail, scaled down to [`FILES_THUMBNAIL_PIXELS`] if
    /// it's bigger, and returns where.
    pub fn store(&self, original: &Original, made: &Path) -> Result<PathBuf, Failed> {
        let picture = read_png(made).map_err(Failed::File)?;
        let picture = picture.fit_within(FILES_THUMBNAIL_PIXELS);
        let path = self.thumbnail_path(original);
        let png = encode(&picture, original).map_err(Failed::Here)?;
        self.write(&path, &png).map_err(Failed::Here)?;
        Ok(path)
    }

    /// Marks `original` as failed, so it isn't tried again until it
    /// changes.
    pub fn mark_failed(&self, original: &Original) -> Result<(), String> {
        let empty = Rgba { width: 1, height: 1, pixels: vec![0; 4] };
        self.write(&self.fail_path(original), &encode(&empty, original)?)
    }

    /// Where a thumbnailer may write its picture for `original` before it's
    /// stored: beside the thumbnails, under a name no other app uses.
    pub fn scratch_path(&self, original: &Original, worker: usize) -> Result<PathBuf, String> {
        let folder = self.root.join(LARGE);
        self.make_folder(&folder)?;
        let (pid, instance) = (std::process::id(), self.instance);
        Ok(folder.join(format!(".{OWN_FAIL}-{pid}-{instance}-{worker}-{}", name(&original.uri))))
    }

    fn thumbnail_path(&self, original: &Original) -> PathBuf {
        self.root.join(LARGE).join(name(&original.uri))
    }

    fn fail_path(&self, original: &Original) -> PathBuf {
        self.root.join(FAIL).join(OWN_FAIL).join(name(&original.uri))
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        if let Some(folder) = path.parent() {
            self.make_folder(folder)?;
        }
        crate::state_file::write(path, bytes).map_err(|e| format!("can't write {}: {e}", path.display()))
    }

    /// Makes `folder` (inside the cache) and those above it up to the
    /// cache, private.
    fn make_folder(&self, folder: &Path) -> Result<(), String> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(FOLDER_MODE)
            .create(folder)
            .map_err(|e| format!("can't make {}: {e}", folder.display()))
    }
}

/// The cache's file name for `uri`.
fn name(uri: &str) -> String {
    let digest = Md5::digest(uri.as_bytes());
    let mut name: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    name.push_str(".png");
    name
}

/// Whether the PNG at `path` is a thumbnail (or failure mark) of `original`
/// as it is now: its URI and modification time match, and its size when
/// the PNG says (a quick save within the same second changes only that).
fn describes(path: &Path, original: &Original) -> bool {
    let Ok(file) = std::fs::File::open(path) else { return false };
    let Ok(reader) = png::Decoder::new(BufReader::new(file)).read_info() else { return false };
    let info = reader.info();
    let text = |key: &str| {
        info.uncompressed_latin1_text
            .iter()
            .find(|chunk| chunk.keyword == key)
            .map(|chunk| chunk.text.clone())
            .or_else(|| {
                info.compressed_latin1_text.iter().find(|chunk| chunk.keyword == key).and_then(|c| c.get_text().ok())
            })
            .or_else(|| info.utf8_text.iter().find(|chunk| chunk.keyword == key).and_then(|c| c.get_text().ok()))
    };
    let mtime = text(MTIME_KEY).and_then(|mtime| mtime.trim().parse::<f64>().ok());
    // Some apps write fractions of a second; the standard's whole seconds count.
    #[allow(clippy::cast_possible_truncation)]
    let same_time = mtime.is_some_and(|mtime| mtime.trunc() as i64 == original.mtime);
    let size = text(SIZE_KEY).and_then(|size| size.trim().parse::<u64>().ok());
    let same_size = size.is_none_or(|size| size == original.size);
    same_time && same_size && text(URI_KEY).as_deref() == Some(original.uri.as_str())
}

/// A picture as 8-bit RGBA rows.
#[derive(Debug, Clone, PartialEq)]
struct Rgba {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Rgba {
    /// This picture, scaled down (averaging the pixels each new one
    /// covers) so neither side is over `most`; as it is when it fits.
    fn fit_within(self, most: u32) -> Self {
        let longer = self.width.max(self.height);
        if longer <= most {
            return self;
        }
        let scale = |side: u32| (u64::from(side) * u64::from(most)).div_ceil(u64::from(longer)).max(1);
        let (width, height) = (scale(self.width), scale(self.height));
        let (old_width, old_height) = (u64::from(self.width), u64::from(self.height));
        let mut pixels = Vec::with_capacity(usize::try_from(width * height * 4).unwrap_or(0));
        for y in 0..height {
            let (top, bottom) =
                (y * old_height / height, ((y + 1) * old_height / height).max(y * old_height / height + 1));
            for x in 0..width {
                let (left, right) =
                    (x * old_width / width, ((x + 1) * old_width / width).max(x * old_width / width + 1));
                let mut sum = [0u64; 4];
                for old_y in top..bottom.min(old_height) {
                    for old_x in left..right.min(old_width) {
                        let at = usize::try_from((old_y * old_width + old_x) * 4).unwrap_or(usize::MAX);
                        for (channel, total) in sum.iter_mut().enumerate() {
                            *total += u64::from(self.pixels.get(at + channel).copied().unwrap_or(0));
                        }
                    }
                }
                let count = (bottom.min(old_height) - top) * (right.min(old_width) - left);
                pixels.extend(sum.map(|total| u8::try_from(total / count.max(1)).unwrap_or(u8::MAX)));
            }
        }
        let side = |value: u64| u32::try_from(value).unwrap_or(most);
        Self { width: side(width), height: side(height), pixels }
    }
}

/// The PNG at `path`, as 8-bit RGBA.
fn read_png(path: &Path) -> Result<Rgba, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("can't read what it made: {e}"))?;
    let failed = |e: png::DecodingError| format!("what it made isn't a PNG: {e}");
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: MAX_DECODE_BYTES });
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(failed)?;
    let size = reader.output_buffer_size().ok_or("what it made is too big")?;
    let mut buffer = vec![0; size];
    let frame = reader.next_frame(&mut buffer).map_err(failed)?;
    buffer.truncate(frame.buffer_size());
    let pixels = match frame.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, u8::MAX]).collect(),
        png::ColorType::GrayscaleAlpha => buffer.as_chunks::<2>().0.iter().flat_map(|&[g, a]| [g, g, g, a]).collect(),
        png::ColorType::Grayscale => buffer.iter().flat_map(|&g| [g, g, g, u8::MAX]).collect(),
        png::ColorType::Indexed => return Err("what it made has a palette left after expanding it".into()),
    };
    Ok(Rgba { width: frame.width, height: frame.height, pixels })
}

/// `picture` as a PNG with `original`'s text chunks.
fn encode(picture: &Rgba, original: &Original) -> Result<Vec<u8>, String> {
    let failed = |e: png::EncodingError| format!("can't encode the thumbnail: {e}");
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, picture.width, picture.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    for (key, value) in [
        (URI_KEY, original.uri.clone()),
        (MTIME_KEY, original.mtime.to_string()),
        (SIZE_KEY, original.size.to_string()),
        (SOFTWARE_KEY, SOFTWARE.to_string()),
    ] {
        encoder.add_text_chunk(key.to_string(), value).map_err(failed)?;
    }
    let mut writer = encoder.write_header().map_err(failed)?;
    writer.write_image_data(&picture.pixels).map_err(failed)?;
    writer.finish().map_err(failed)?;
    Ok(png)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use nova_test_support::TempDir;

    /// A `width`×`height` RGB PNG, every pixel `color`, as a thumbnailer
    /// writes one.
    pub fn rgb_png(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let pixels: Vec<u8> = (0..width * height).flat_map(|_| color).collect();
        writer.write_image_data(&pixels).unwrap();
        writer.finish().unwrap();
        png
    }

    fn original(dir: &TempDir, name: &str) -> Original {
        let path = dir.join(name);
        std::fs::write(&path, b"picture").unwrap();
        Original::new(&path, &std::fs::metadata(&path).unwrap())
    }

    #[test]
    fn uris_and_names_match_what_glib_writes() {
        // `g_filename_to_uri` and the MD5 GNOME files its thumbnail under.
        let path = Path::new("/home/jens/photos/me.png");
        assert_eq!(uri(path), "file:///home/jens/photos/me.png");
        assert_eq!(name(&uri(path)), "c6ee772d9e49320e97ec29a7eb5b1697.png");
        let odd = Path::new(std::ffi::OsStr::from_bytes(b"/a b/caf\xc3\xa9 #1;(x)~&.png"));
        assert_eq!(uri(odd), "file:///a%20b/caf%C3%A9%20%231%3B(x)~&.png");
    }

    #[test]
    fn a_stored_thumbnail_is_found_until_the_file_changes() {
        let dir = TempDir::new("thumbnail-cache");
        let cache = Cache::new(dir.join("cache/thumbnails"));
        let photo = original(&dir, "photo.jpg");
        assert_eq!(cache.found(&photo), None);
        std::fs::write(dir.join("made.png"), rgb_png(4, 2, [200, 10, 10])).unwrap();
        let stored = cache.store(&photo, &dir.join("made.png")).unwrap();
        assert!(stored.starts_with(dir.join("cache/thumbnails/large")));
        assert_eq!(cache.found(&photo), Some(stored.clone()));
        let mode = |path: &Path| std::fs::metadata(path).unwrap().mode() & 0o777;
        assert_eq!(mode(&stored), 0o600);
        assert_eq!(mode(&dir.join("cache/thumbnails/large")), FOLDER_MODE);
        let read = read_png(&stored).unwrap();
        assert_eq!((read.width, read.height), (4, 2));
        assert_eq!(&read.pixels[..4], &[200, 10, 10, 255]);

        let changed = Original { mtime: photo.mtime + 60, ..photo.clone() };
        assert_eq!(cache.found(&changed), None, "a thumbnail of the file as it was is stale");
        let resaved = Original { size: photo.size + 1, ..photo.clone() };
        assert_eq!(cache.found(&resaved), None, "saved again within the second, it's stale too");
    }

    #[test]
    fn a_failure_is_remembered_for_the_file_as_it_is() {
        let dir = TempDir::new("thumbnail-cache-fail");
        let cache = Cache::new(dir.join("thumbnails"));
        let broken = original(&dir, "broken.jpg");
        assert!(!cache.failed(&broken));
        cache.mark_failed(&broken).unwrap();
        assert!(cache.failed(&broken));
        assert_eq!(cache.found(&broken), None);
        assert!(!cache.failed(&Original { mtime: broken.mtime + 1, ..broken.clone() }));
        assert!(!cache.failed(&Original { size: broken.size + 1, ..broken }));
    }

    #[test]
    fn a_big_picture_is_scaled_to_fit_and_a_broken_one_refused() {
        let dir = TempDir::new("thumbnail-cache-big");
        let cache = Cache::new(dir.join("thumbnails"));
        let poster = original(&dir, "poster.mkv");
        std::fs::write(dir.join("made.png"), rgb_png(1024, 512, [0, 0, 255])).unwrap();
        let stored = read_png(&cache.store(&poster, &dir.join("made.png")).unwrap()).unwrap();
        assert_eq!((stored.width, stored.height), (FILES_THUMBNAIL_PIXELS, FILES_THUMBNAIL_PIXELS / 2));
        assert_eq!(&stored.pixels[..4], &[0, 0, 255, 255]);
        std::fs::write(dir.join("junk.png"), b"not a png").unwrap();
        assert!(cache.store(&poster, &dir.join("junk.png")).is_err());
    }

    #[test]
    fn scaling_averages_what_each_pixel_covers() {
        let picture = Rgba { width: 4, height: 1, pixels: [[0, 0, 0, 255], [255, 255, 255, 255]].repeat(2).concat() };
        let half = picture.fit_within(2);
        assert_eq!((half.width, half.height), (2, 1));
        assert_eq!(half.pixels, [127, 127, 127, 255, 127, 127, 127, 255]);
    }

    #[test]
    fn the_cache_holds_its_own_files_only() {
        let cache = Cache::new(PathBuf::from("/home/me/.cache/thumbnails"));
        assert!(cache.holds(Path::new("/home/me/.cache/thumbnails/large/x.png")));
        assert!(!cache.holds(Path::new("/home/me/.cache/thumbnailsx/y.png")));
    }

    #[test]
    fn the_cache_holds_its_files_reached_through_links() {
        let dir = TempDir::new("thumbnail-cache-links");
        std::fs::create_dir_all(dir.join("real/large")).unwrap();
        std::fs::write(dir.join("real/large/x.png"), b"").unwrap();
        std::os::unix::fs::symlink(dir.join("real/large"), dir.join("shortcut")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("cache")).unwrap();
        let cache = Cache::new(dir.join("real"));
        assert!(cache.holds(&dir.join("shortcut/x.png")), "a link into the cache");
        assert!(Cache::new(dir.join("cache")).holds(&dir.join("real/large/x.png")), "a cache that's a link");
        std::fs::write(dir.join("y.png"), b"").unwrap();
        assert!(!cache.holds(&dir.join("y.png")));
    }

    #[test]
    fn caches_opened_again_never_share_scratch_files() {
        let dir = TempDir::new("thumbnail-cache-scratch");
        let photo = original(&dir, "photo.jpg");
        let first = Cache::new(dir.join("thumbnails")).scratch_path(&photo, 0).unwrap();
        let again = Cache::new(dir.join("thumbnails")).scratch_path(&photo, 0).unwrap();
        assert_ne!(first, again);
    }
}
