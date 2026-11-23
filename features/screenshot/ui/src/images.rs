//! The pictures the picker and the card draw, read off the main thread:
//! a frozen screen at its own size, and a saved shot made small for the
//! card. Each is its own `RenderImage`, not GPUI's `img(path)`: GPUI keeps
//! an image loaded by its path for good, and novad writes every capture of
//! a screen to the same path, so the next pick would show the last one's
//! frame. The owner drops each image from the windows when it is done
//! with it.

use gpui::RenderImage;
use image::{Frame, RgbaImage};
use std::path::Path;
use std::sync::Arc;

/// A frozen screen (`grim`'s PPM, or any image), at its own size.
pub fn frame(path: &Path) -> Result<Arc<RenderImage>, String> {
    let image = image::open(path).map_err(|error| format!("can't read {}: {error}", path.display()))?;
    Ok(render(image.into_rgba8()))
}

/// A saved shot, made at most `width` by `height` pixels for the card.
pub fn thumbnail(path: &Path, width: u32, height: u32) -> Result<Arc<RenderImage>, String> {
    let image = image::open(path).map_err(|error| format!("can't read {}: {error}", path.display()))?;
    let small = if image.width() > width || image.height() > height { image.thumbnail(width, height) } else { image };
    Ok(render(small.into_rgba8()))
}

/// GPUI draws images from BGRA.
fn render(mut pixels: RgbaImage) -> Arc<RenderImage> {
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![Frame::new(pixels)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_reads_grim_s_ppm_into_bgra() {
        let folder = std::env::temp_dir().join(format!("nova-screenshot-ui-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("DP-1.ppm");
        // Two pixels: red, then blue.
        let mut ppm = b"P6\n2 1\n255\n".to_vec();
        ppm.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
        std::fs::write(&path, ppm).unwrap();
        let image = frame(&path).unwrap();
        assert_eq!(image.as_bytes(0).unwrap(), &[0, 0, 255, 255, 255, 0, 0, 255]);
        let small = thumbnail(&path, 1, 1).unwrap();
        assert_eq!(small.size(0).width.0, 1);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_missing_file_says_which() {
        let error = frame(Path::new("/nonexistent/DP-1.ppm")).err().unwrap_or_default();
        assert!(error.contains("/nonexistent/DP-1.ppm"), "{error}");
    }
}
