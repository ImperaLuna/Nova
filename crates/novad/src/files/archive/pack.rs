//! Writing a zip of files and folders ("Compress to ZIP"): each item under
//! its own name, folders with everything in them, links as links. Files
//! are deflated and keep their permissions and time; one too big for a
//! plain zip is written as zip64. Devices and pipes are left out.

use std::fs::{self, File, Metadata};
use std::io::{self, Read, Seek, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// Bytes copied between checks for a stop.
const CHUNK_BYTES: usize = 1 << 20;
/// A file this big or bigger needs zip64.
const ZIP64_BYTES: u64 = u32::MAX as u64;

/// Writes a zip of `sources` to `out`. `progress` hears each chunk's
/// bytes, and answers false to stop; then this answers false.
pub fn pack<W: Write + Seek>(sources: &[&Path], out: W, progress: &mut dyn FnMut(u64) -> bool) -> io::Result<bool> {
    let mut zip = ZipWriter::new(out);
    for source in sources {
        let Some(name) = source.file_name() else { continue };
        if !add(&mut zip, source, &name.to_string_lossy(), progress)? {
            return Ok(false);
        }
    }
    zip.finish().map_err(io::Error::other)?;
    Ok(true)
}

/// Adds `path` to `zip` as `name`, and what's in it for a folder.
fn add<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    path: &Path,
    name: &str,
    progress: &mut dyn FnMut(u64) -> bool,
) -> io::Result<bool> {
    let meta = fs::symlink_metadata(path)?;
    let options = options(&meta);
    let kind = meta.file_type();
    if kind.is_symlink() {
        let target = fs::read_link(path)?;
        zip.add_symlink(name, target.to_string_lossy(), options).map_err(io::Error::other)?;
    } else if kind.is_dir() {
        zip.add_directory(format!("{name}/"), options).map_err(io::Error::other)?;
        let mut children: Vec<_> = fs::read_dir(path)?.map(|entry| entry.map(|entry| entry.file_name())).collect::<io::Result<_>>()?;
        // The same zip from the same folder.
        children.sort();
        for child in children {
            let inner = format!("{name}/{}", child.to_string_lossy());
            if !add(zip, &path.join(&child), &inner, progress)? {
                return Ok(false);
            }
        }
    } else if kind.is_file() {
        zip.start_file(name, options.large_file(meta.len() >= ZIP64_BYTES)).map_err(io::Error::other)?;
        let mut file = File::open(path)?;
        let mut buffer = vec![0; CHUNK_BYTES];
        loop {
            let read = match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            zip.write_all(&buffer[..read])?;
            if !progress(read as u64) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// How an item with `meta` is stored: deflated, with its permissions and
/// time (a time a zip can't hold, before 1980, is left out).
fn options(meta: &Metadata) -> SimpleFileOptions {
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(meta.permissions().mode() & 0o777);
    let modified = meta.modified().ok().and_then(|time| {
        let stamp = jiff::Timestamp::try_from(time).ok()?;
        zip::DateTime::try_from(stamp.to_zoned(jiff::tz::TimeZone::system()).datetime()).ok()
    });
    match modified {
        Some(modified) => options.last_modified_time(modified),
        None => options,
    }
}
