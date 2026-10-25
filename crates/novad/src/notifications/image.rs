//! A notification's picture, as an app sends it: raw pixels in an
//! `image-data` hint, written out as a PNG the UI can load; or a file, by
//! path or `file://` URI, which the UI loads as it is.

use crate::state_file;
use std::path::{Path, PathBuf};

/// The pixels in an `image-data` hint: width, height, row stride, whether
/// there's an alpha channel, bits per sample, channels, and the bytes.
pub type Raw = (i32, i32, i32, bool, i32, i32, Vec<u8>);

/// Pictures bigger than this on either side are refused: senders scale
/// avatars down, and anything on the bus can send one.
const MAX_SIDE: u32 = 1024;

/// Writes `raw` to `<folder>/<id>.png`, replacing what was there, and
/// returns that path.
pub fn save(raw: Raw, folder: &Path, id: u32) -> Result<PathBuf, String> {
    let (width, height, stride, alpha, bits, channels, bytes) = raw;
    let (Ok(width), Ok(height), Ok(stride)) = (u32::try_from(width), u32::try_from(height), usize::try_from(stride))
    else {
        return Err("negative sizes".into());
    };
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!("{width}×{height} isn't between 1×1 and {MAX_SIDE}×{MAX_SIDE}"));
    }
    let wanted = if alpha { 4 } else { 3 };
    if bits != 8 || channels != wanted {
        return Err(format!("{channels} channels of {bits} bits aren't 8-bit RGB or RGBA"));
    }
    let row = width as usize * wanted as usize;
    // The last row may stop short of the stride.
    if stride < row || bytes.len() < stride * (height as usize - 1) + row {
        return Err("fewer bytes than its size needs".into());
    }
    let pixels: Vec<u8> = bytes.chunks(stride).take(height as usize).flat_map(|line| &line[..row]).copied().collect();

    let failed = |e: png::EncodingError| format!("can't encode it: {e}");
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, width, height);
    encoder.set_color(if alpha { png::ColorType::Rgba } else { png::ColorType::Rgb });
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(failed)?;
    writer.write_image_data(&pixels).map_err(failed)?;
    writer.finish().map_err(failed)?;

    let path = saved(folder, id);
    state_file::write(&path, &png).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    Ok(path)
}

/// Where `save` writes notification `id`'s picture: novad deletes a picture
/// only at this path, never one an app named.
pub fn saved(folder: &Path, id: u32) -> PathBuf {
    folder.join(format!("{id}.png"))
}

/// The file `reference` names, for an `image-path` hint or an app icon: an
/// absolute path, or a `file://` URI. `None` for an icon name.
pub fn file(reference: &str) -> Option<PathBuf> {
    let path = reference.strip_prefix("file://").unwrap_or(reference);
    let path = Path::new(path);
    path.is_absolute().then(|| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padded_rows_become_a_png_of_the_right_size() {
        let dir = std::env::temp_dir().join(format!("novad-notification-image-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // 2×2 RGB, each row padded to 8 bytes; the last row isn't.
        let bytes = vec![255, 0, 0, 0, 255, 0, 9, 9, 0, 0, 255, 255, 255, 255];
        let path = save((2, 2, 8, false, 8, 3, bytes), &dir, 5).unwrap();
        assert_eq!(path, dir.join("5.png"));
        let mut reader = png::Decoder::new(std::io::Cursor::new(std::fs::read(&path).unwrap())).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!(&pixels[..12], &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn odd_pixels_are_refused() {
        let dir = std::env::temp_dir();
        assert!(save((2, 2, 6, false, 8, 3, vec![0; 6]), &dir, 1).unwrap_err().contains("fewer bytes"));
        assert!(save((2, 2, 8, true, 8, 3, vec![0; 16]), &dir, 1).is_err());
        assert!(save((4096, 1, 12288, false, 8, 3, vec![0; 12288]), &dir, 1).is_err());
        assert!(save((-1, 1, 3, false, 8, 3, vec![0; 3]), &dir, 1).is_err());
    }

    #[test]
    fn files_are_paths_or_file_uris() {
        assert_eq!(file("file:///tmp/a.png"), Some(PathBuf::from("/tmp/a.png")));
        assert_eq!(file("/tmp/a.png"), Some(PathBuf::from("/tmp/a.png")));
        assert_eq!(file("discord"), None);
        assert_eq!(file("relative/a.png"), None);
    }
}
