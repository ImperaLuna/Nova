//! A notification's picture, as an app sends it: raw pixels in an
//! `image-data` hint, written out as a PNG the UI can load; or a file, by
//! path or `file://` URI (`nova_paths::reference`), which the UI loads as it is.

use nova_base::state_file;
use std::path::{Path, PathBuf};

/// The pixels in an `image-data` hint: width, height, row stride, whether
/// there's an alpha channel, bits per sample, channels, and the bytes.
pub type Raw = (i32, i32, i32, bool, i32, i32, Vec<u8>);

/// Pictures bigger than this on either side are refused: senders scale
/// avatars down, and anything on the bus can send one.
const MAX_SIDE: u32 = 1024;

/// Writes `raw` to `<folder>/<id>-<revision>.png` and returns that path. A
/// new revision for each picture gives it a path of its own, so the UI
/// loads it afresh rather than showing a cached one.
pub fn save(raw: Raw, folder: &Path, id: u32, revision: u64) -> Result<PathBuf, String> {
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

    let path = folder.join(format!("{id}-{revision}.png"));
    state_file::write(&path, &png).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    Ok(path)
}

/// Whether `path` is a picture `save` wrote for notification `id`: novad
/// deletes only those, never a file an app named.
pub fn is_saved(folder: &Path, id: u32, path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else { return false };
    let revision = name.strip_prefix(&format!("{id}-")).and_then(|rest| rest.strip_suffix(".png"));
    path.parent() == Some(folder) && revision.is_some_and(|revision| revision.parse::<u64>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padded_rows_become_a_png_of_the_right_size() {
        let dir = nova_test_support::TempDir::new("notification-image");
        // 2×2 RGB, each row padded to 8 bytes; the last row isn't.
        let bytes = vec![255, 0, 0, 0, 255, 0, 9, 9, 0, 0, 255, 255, 255, 255];
        let path = save((2, 2, 8, false, 8, 3, bytes), &dir, 5, 9).unwrap();
        assert_eq!(path, dir.join("5-9.png"));
        assert!(is_saved(&dir, 5, &path));
        assert!(!is_saved(&dir, 6, &path));
        assert!(!is_saved(&dir, 5, &dir.join("../5-9.png")));
        assert!(!is_saved(&dir, 5, &dir.join("5-9.png/../../victim")));
        let mut reader = png::Decoder::new(std::io::Cursor::new(std::fs::read(&path).unwrap())).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!(&pixels[..12], &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
    }

    #[test]
    fn odd_pixels_are_refused() {
        let dir = std::env::temp_dir();
        assert!(save((2, 2, 6, false, 8, 3, vec![0; 6]), &dir, 1, 0).unwrap_err().contains("fewer bytes"));
        assert!(save((2, 2, 8, true, 8, 3, vec![0; 16]), &dir, 1, 0).is_err());
        assert!(save((4096, 1, 12288, false, 8, 3, vec![0; 12288]), &dir, 1, 0).is_err());
        assert!(save((-1, 1, 3, false, 8, 3, vec![0; 3]), &dir, 1, 0).is_err());
    }
}
