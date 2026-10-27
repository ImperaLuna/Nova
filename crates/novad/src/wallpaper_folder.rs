//! The wallpaper folder (feature `wallpaper`): the images the carousel
//! offers, from config.toml's `[wallpaper] folder`.
//!
//! Listed afresh each time the carousel opens, so a picture dropped in shows
//! next time without watching the folder. Only the folder's own files count,
//! not its subfolders or hidden files, and only images both awww and the UI
//! can draw, told by their header rather than their name.

use nova_protocol::{WALLPAPER_FOLDER_MAX_IMAGES, WallpaperFolder};
use std::path::{Path, PathBuf};

/// The images in `folder`, sorted by name; none when it doesn't exist yet.
pub fn list(folder: &Path) -> Result<WallpaperFolder, String> {
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(WallpaperFolder { folder: folder.display().to_string(), images: vec![] });
        }
        Err(e) => return Err(format!("can't list {}: {e}", folder.display())),
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name| !name.starts_with('.')))
        .collect();
    files.sort_by_key(|path| sort_key(path));
    // Sorted first, so only as many files are opened as it takes to find
    // the most it lists.
    let images: Vec<PathBuf> =
        files.into_iter().filter(|path| is_image(path)).take(WALLPAPER_FOLDER_MAX_IMAGES).collect();
    Ok(WallpaperFolder {
        folder: folder.display().to_string(),
        images: images.iter().map(|path| path.display().to_string()).collect(),
    })
}

/// Whether `path` is a file (or a link to one) whose header says it's an
/// image of a kind awww and Qt both draw.
fn is_image(path: &Path) -> bool {
    // Looked at before it's opened: opening a FIFO waits for a writer.
    if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    use imagesize::ImageType;
    matches!(
        imagesize::reader_type(std::io::BufReader::new(file)),
        Ok(ImageType::Jpeg | ImageType::Png | ImageType::Webp | ImageType::Gif | ImageType::Bmp)
    )
}

/// Names compared without case, as a file manager lists them.
fn sort_key(path: &Path) -> (String, PathBuf) {
    let name = path.file_name().map(|name| name.to_string_lossy().to_lowercase()).unwrap_or_default();
    (name, path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    fn png(path: &Path) {
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(file, 1, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.write_header().unwrap().write_image_data(&[0, 0, 0]).unwrap();
    }

    #[test]
    fn a_missing_folder_has_no_images() {
        let dir = TempDir::new("wallpaper-folder-missing");
        let listed = list(&dir.join("Wallpapers")).unwrap();
        assert!(listed.images.is_empty());
        assert!(listed.folder.ends_with("Wallpapers"));
    }

    #[test]
    fn only_the_folders_own_images_are_listed_by_name() {
        let dir = TempDir::new("wallpaper-folder-images");
        let folder = dir.join("Wallpapers");
        std::fs::create_dir_all(folder.join("old")).unwrap();
        png(&folder.join("tide.png"));
        png(&folder.join("Dusk.png"));
        // An image whatever its name, and none whatever its name.
        png(&folder.join("aurora"));
        std::fs::write(folder.join("notes.jpg"), "not an image").unwrap();
        png(&folder.join(".hidden.png"));
        png(&folder.join("old/ember.png"));
        std::os::unix::fs::symlink(folder.join("tide.png"), folder.join("linked.png")).unwrap();
        std::os::unix::fs::symlink(folder.join("gone.png"), folder.join("broken.png")).unwrap();
        let names: Vec<String> = list(&folder)
            .unwrap()
            .images
            .iter()
            .map(|path| Path::new(path).file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["aurora", "Dusk.png", "linked.png", "tide.png"]);
    }

    #[test]
    fn a_file_is_not_a_folder() {
        let dir = TempDir::new("wallpaper-folder-file");
        let file = dir.join("Wallpapers");
        std::fs::write(&file, "").unwrap();
        assert!(list(&file).unwrap_err().contains("can't list"));
    }
}
