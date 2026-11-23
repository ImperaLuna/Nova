//! What a tray item shows, read from its `org.kde.StatusNotifierItem`
//! properties for the GPUI UI, which has no tray host of its own: its
//! picture as a file, whether it asks for attention, and its menu's place.
//!
//! A picture comes as an icon name or as pixels. A name is an absolute
//! path, a file in the item's own folder (`IconThemePath`, Spotify's), or
//! a name in the icon theme (`nova_icons`). Pixels (Electron's) are
//! written out as a PNG in the tray's folder under the runtime dir, named
//! by their content so a new picture gets a new path, and removed when the
//! item changes its picture or goes. While the item asks for attention its
//! attention picture is used, when it has one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use zbus::OwnedVariant as OwnedValue;

/// `Status` of an item that asks for attention.
const NEEDS_ATTENTION: &str = "NeedsAttention";
/// What `Menu` holds for an item without one: no path, the root, or
/// libappindicator's stand-in.
const NO_MENU: [&str; 3] = ["", "/", "/NO_DBUSMENU"];
/// Pictures bigger than this on either side are skipped: tray icons are
/// tiny, and anything on the bus can send one.
const MAX_SIDE: i32 = 256;
/// The picture sizes an item may send at most; past it, the rest are skipped.
const MAX_PIXMAPS: usize = 16;
/// Image types looked for in an item's own folder, as `nova_icons` prefers them.
const EXTENSIONS: [&str; 2] = ["png", "svg"];
/// How deep into an item's own folder a name is looked for: Spotify's is
/// flat, a theme laid out inside one is `hicolor/<size>/apps`.
const MAX_FOLDER_DEPTH: usize = 3;

/// One of an item's pictures as it sends it: width, height and ARGB32
/// pixels in network byte order.
pub type Pixmap = (i32, i32, Vec<u8>);

/// The properties the look is read from, as `GetAll` answers.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Props {
    pub status: String,
    pub icon_name: String,
    pub icon_pixmap: Vec<Pixmap>,
    pub attention_icon_name: String,
    pub attention_icon_pixmap: Vec<Pixmap>,
    pub icon_theme_path: String,
    pub item_is_menu: bool,
    pub menu: String,
}

impl Props {
    /// Picks the properties out of what `GetAll` answered. Anything of the
    /// wrong type is left as if the item hadn't sent it.
    pub fn from_all(all: HashMap<String, OwnedValue>) -> Self {
        let text =
            |name: &str| all.get(name).and_then(|value| String::try_from(value.clone()).ok()).unwrap_or_default();
        let path = |name: &str| {
            all.get(name)
                .and_then(|value| zbus::OwnedObjectPath::try_from(value.clone()).ok())
                .map(|path| path.to_string())
                .unwrap_or_default()
        };
        let pixmaps = |name: &str| {
            all.get(name)
                .and_then(|value| Vec::<Pixmap>::try_from(value.clone()).ok())
                .map(|mut list| {
                    list.truncate(MAX_PIXMAPS);
                    list
                })
                .unwrap_or_default()
        };
        Self {
            status: text("Status"),
            icon_name: text("IconName"),
            icon_pixmap: pixmaps("IconPixmap"),
            attention_icon_name: text("AttentionIconName"),
            attention_icon_pixmap: pixmaps("AttentionIconPixmap"),
            icon_theme_path: text("IconThemePath"),
            item_is_menu: all.get("ItemIsMenu").and_then(|value| bool::try_from(value).ok()).unwrap_or(false),
            menu: path("Menu"),
        }
    }

    pub fn attention(&self) -> bool {
        self.status == NEEDS_ATTENTION
    }

    /// Where its menu is on its connection, when it has one.
    pub fn menu_path(&self) -> Option<String> {
        (!NO_MENU.contains(&self.menu.as_str())).then(|| self.menu.clone())
    }

    /// The name and pixels to draw now: the attention ones while it asks
    /// for attention and has any.
    fn picture(&self) -> (&str, &[Pixmap]) {
        let has_attention_picture = !self.attention_icon_name.is_empty() || !self.attention_icon_pixmap.is_empty();
        if self.attention() && has_attention_picture {
            (&self.attention_icon_name, &self.attention_icon_pixmap)
        } else {
            (&self.icon_name, &self.icon_pixmap)
        }
    }
}

/// What a tray item shows.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Look {
    pub icon: Option<String>,
    pub attention: bool,
    pub menu_path: Option<String>,
    pub is_menu: bool,
    /// The PNG written for its pixels, which is removed with it.
    pub written: Option<PathBuf>,
}

/// Where the look's files are found and written.
pub struct Pictures<'a> {
    pub icons: &'a nova_icons::Icons,
    /// The tray's own folder under the runtime dir.
    pub folder: &'a Path,
}

impl Pictures<'_> {
    /// The look `props` give the item numbered `serial`. A picture the item
    /// had before (`old`) that this one doesn't keep is removed.
    pub fn look(&self, props: &Props, serial: u64, old: Option<&Path>) -> Look {
        let (name, pixmaps) = props.picture();
        let named = self.named(name, &props.icon_theme_path);
        let written = if named.is_some() { None } else { self.write(pixmaps, serial) };
        if let Some(old) = old
            && written.as_deref() != Some(old)
        {
            remove(old);
        }
        let icon = named.or_else(|| written.as_ref().map(|path| path.to_string_lossy().into_owned()));
        Look { icon, attention: props.attention(), menu_path: props.menu_path(), is_menu: props.item_is_menu, written }
    }

    /// The file for icon `name`, from the item's own folder or the theme.
    fn named(&self, name: &str, folder: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        let path = Path::new(name);
        if path.is_absolute() {
            return path.is_file().then(|| name.to_string());
        }
        if !folder.is_empty()
            && is_plain_name(name)
            && let Some(found) = in_folder(Path::new(folder), name, MAX_FOLDER_DEPTH)
        {
            return Some(found.to_string_lossy().into_owned());
        }
        self.icons.find(name)
    }

    /// Writes the largest of `pixmaps` as a PNG and gives its path.
    fn write(&self, pixmaps: &[Pixmap], serial: u64) -> Option<PathBuf> {
        let (width, height, rgba) = largest(pixmaps)?;
        let png = encode(width, height, &rgba)?;
        let path = self.folder.join(format!("{serial}-{:016x}.png", content_hash(&png)));
        if path.is_file() {
            return Some(path);
        }
        std::fs::create_dir_all(self.folder).ok()?;
        nova_base::state_file::write(&path, &png).ok()?;
        Some(path)
    }
}

/// The largest picture that is whole and not too big, as RGBA.
pub fn largest(pixmaps: &[Pixmap]) -> Option<(u32, u32, Vec<u8>)> {
    let fits = |(width, height, pixels): &&Pixmap| {
        (1..=MAX_SIDE).contains(width)
            && (1..=MAX_SIDE).contains(height)
            && i64::try_from(pixels.len()).is_ok_and(|len| len == i64::from(*width) * i64::from(*height) * 4)
    };
    let (width, height, argb) = pixmaps.iter().filter(fits).max_by_key(|(width, height, _)| width * height)?;
    let rgba = argb.chunks_exact(4).flat_map(|pixel| [pixel[1], pixel[2], pixel[3], pixel[0]]).collect();
    Some((u32::try_from(*width).ok()?, u32::try_from(*height).ok()?, rgba))
}

fn encode(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;
    Some(png)
}

fn content_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// `<name>.png` or `.svg` in `folder` or the folders under it, `depth`
/// deep at most, the shallowest first.
fn in_folder(folder: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    let here =
        EXTENSIONS.iter().map(|extension| folder.join(format!("{name}.{extension}"))).find(|path| path.is_file());
    if here.is_some() || depth == 0 {
        return here;
    }
    let mut folders: Vec<PathBuf> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    // Bigger sizes first, as theme folders are named (`48x48` before `16x16`).
    folders.sort_by_key(|path| std::cmp::Reverse(leading_number(path)));
    folders.into_iter().find_map(|inner| in_folder(&inner, name, depth - 1))
}

/// A name that stays inside the folder it is looked for in.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255 && !name.starts_with('.') && !name.contains(['/', '\\', '\0'])
}

fn leading_number(path: &Path) -> u32 {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    name.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)
}

/// Removes a PNG the tray wrote.
pub fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Empties the tray's folder, at start: what an earlier novad wrote is
/// nobody's now.
pub fn clear(folder: &Path) {
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        if entry.path().extension().is_some_and(|extension| extension == "png") {
            remove(&entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    fn icons(dir: &Path) -> nova_icons::Icons {
        let hicolor = dir.join("share/icons/hicolor");
        std::fs::create_dir_all(&hicolor).unwrap();
        std::fs::write(
            hicolor.join("index.theme"),
            "[Icon Theme]\nDirectories=48x48/apps\n[48x48/apps]\nSize=48\nType=Fixed\n",
        )
        .unwrap();
        nova_icons::Icons::new(nova_icons::HICOLOR.into(), &[dir.join("share")], None)
    }

    #[test]
    fn pixels_become_a_png_named_by_their_content() {
        let dir = TempDir::new("tray-look-pixels");
        let icons = icons(&dir);
        let pictures = Pictures { icons: &icons, folder: &dir.join("tray") };
        // 1×1 opaque red, and a 2×1 one that wins as the larger.
        let props = Props {
            icon_pixmap: vec![(1, 1, vec![255, 255, 0, 0]), (2, 1, vec![255, 0, 255, 0, 128, 0, 0, 255])],
            ..Props::default()
        };
        let look = pictures.look(&props, 7, None);
        let written = look.written.clone().unwrap();
        assert_eq!(look.icon.as_deref(), written.to_str());
        assert!(written.file_name().unwrap().to_str().unwrap().starts_with("7-"));
        let mut reader = png::Decoder::new(std::io::Cursor::new(std::fs::read(&written).unwrap())).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (2, 1));
        assert_eq!(&pixels[..8], &[0, 255, 0, 255, 0, 0, 255, 128], "ARGB became RGBA");

        let again = Props { icon_pixmap: vec![(1, 1, vec![255, 0, 0, 255])], ..Props::default() };
        let newer = pictures.look(&again, 7, Some(&written));
        assert_ne!(newer.written, Some(written.clone()));
        assert!(!written.exists(), "the old picture is removed");
        clear(&dir.join("tray"));
        assert!(!newer.written.unwrap().exists());
    }

    #[test]
    fn a_name_is_found_in_the_item_s_own_folder_before_the_theme() {
        let dir = TempDir::new("tray-look-named");
        let own = dir.join("own/hicolor/22x22/apps");
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(own.join("spotify-linux-32.png"), b"png").unwrap();
        let themed = dir.join("share/icons/hicolor/48x48/apps");
        std::fs::create_dir_all(&themed).unwrap();
        std::fs::write(themed.join("steam.png"), b"png").unwrap();
        let icons = icons(&dir);
        let pictures = Pictures { icons: &icons, folder: &dir.join("tray") };
        let spotify = Props {
            icon_name: "spotify-linux-32".into(),
            icon_theme_path: dir.join("own").to_string_lossy().into_owned(),
            ..Props::default()
        };
        assert_eq!(
            pictures.look(&spotify, 1, None).icon,
            Some(own.join("spotify-linux-32.png").to_string_lossy().into())
        );
        let steam = Props { icon_name: "steam".into(), ..Props::default() };
        assert_eq!(pictures.look(&steam, 2, None).icon, Some(themed.join("steam.png").to_string_lossy().into()));
        let missing = Props { icon_name: "nothing-like-it".into(), ..Props::default() };
        assert_eq!(pictures.look(&missing, 3, None).icon, None);
    }

    #[test]
    fn asking_for_attention_shows_the_attention_picture() {
        let dir = TempDir::new("tray-look-attention");
        let folder = dir.join("share/icons/hicolor/48x48/apps");
        std::fs::create_dir_all(&folder).unwrap();
        for name in ["chat", "chat-unread"] {
            std::fs::write(folder.join(format!("{name}.png")), b"png").unwrap();
        }
        let icons = icons(&dir);
        let pictures = Pictures { icons: &icons, folder: &dir.join("tray") };
        let mut props = Props {
            status: "Active".into(),
            icon_name: "chat".into(),
            attention_icon_name: "chat-unread".into(),
            menu: "/MenuBar".into(),
            ..Props::default()
        };
        let calm = pictures.look(&props, 1, None);
        assert!(!calm.attention);
        assert!(calm.icon.unwrap().ends_with("chat.png"));
        assert_eq!(calm.menu_path.as_deref(), Some("/MenuBar"));
        props.status = NEEDS_ATTENTION.into();
        let asking = pictures.look(&props, 1, None);
        assert!(asking.attention);
        assert!(asking.icon.unwrap().ends_with("chat-unread.png"));
    }

    #[test]
    fn odd_pictures_and_menus_are_left_out() {
        let broken = vec![(2, 2, vec![0; 4]), (0, 0, vec![]), (MAX_SIDE + 1, 1, vec![0; 4 * (MAX_SIDE as usize + 1)])];
        assert_eq!(largest(&broken), None);
        for menu in NO_MENU {
            assert_eq!(Props { menu: menu.into(), ..Props::default() }.menu_path(), None);
        }
    }
}
