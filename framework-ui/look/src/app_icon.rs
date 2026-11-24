use crate::Theme;
use crate::tokens::{APP_ICON_SIZE, LETTER_SCALE, LETTER_TILE_RADIUS, WEIGHT_TITLE};
use gpui::{
    AnyElement, App, Asset, AssetLogger, DevicePixels, Hsla, ImageCacheError, ImageSource, IntoElement, ObjectFit,
    ParentElement, Pixels, RenderImage, RenderOnce, SharedString, Styled, StyledImage, SvgRenderer, SvgSize, Window,
    div, img, size,
};
use image::imageops::FilterType;
use image::{DynamicImage, Frame};
use std::future::Future;
use std::sync::Arc;

#[derive(Debug, Clone, IntoElement)]
pub struct AppIcon {
    id: SharedString,
    name: SharedString,
    icon: Option<SharedString>,
    size: Pixels,
    resting: Option<Pixels>,
}

impl AppIcon {
    pub fn new(id: impl Into<SharedString>, name: impl Into<SharedString>) -> Self {
        Self { id: id.into(), name: name.into(), icon: None, size: APP_ICON_SIZE, resting: None }
    }

    pub fn icon(mut self, icon: Option<impl Into<SharedString>>) -> Self {
        self.icon = icon.map(Into::into).filter(|icon| icon.starts_with('/'));
        self
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn resting_size(mut self, size: Pixels) -> Self {
        self.resting = Some(size);
        self
    }
}

impl RenderOnce for AppIcon {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let swatches = &Theme::of(cx).swatches;
        let tile = LetterTile {
            letter: letter(&self.name).into(),
            swatch: swatches[swatch(&self.id, swatches.len())],
            ink: Theme::of(cx).on_accent,
            size: self.size,
        };
        let Some(icon) = self.icon else {
            return tile.build();
        };
        let (loading, missing) = (tile.clone(), tile);
        let logical = self.resting.unwrap_or(self.size);
        let source = ImageSource::Custom(Arc::new(move |window: &mut Window, cx: &mut App| {
            let side = device_side(logical, window.scale_factor());
            window.use_asset::<AssetLogger<SizedIcon>>(&(icon.clone(), side), cx)
        }));
        img(source)
            .size(self.size)
            .flex_none()
            .object_fit(ObjectFit::Contain)
            .with_loading(move || loading.clone().build())
            .with_fallback(move || missing.clone().build())
            .into_any_element()
    }
}

#[derive(Clone)]
struct LetterTile {
    letter: SharedString,
    swatch: Hsla,
    ink: Hsla,
    size: Pixels,
}

impl LetterTile {
    fn build(self) -> AnyElement {
        let radius = LETTER_TILE_RADIUS * (self.size / APP_ICON_SIZE);
        div()
            .size(self.size)
            .flex_none()
            .rounded(radius)
            .bg(self.swatch)
            .flex()
            .items_center()
            .justify_center()
            .text_size((self.size * LETTER_SCALE).round())
            .font_weight(WEIGHT_TITLE)
            .text_color(self.ink)
            .child(self.letter)
            .into_any_element()
    }
}

enum SizedIcon {}

impl Asset for SizedIcon {
    type Source = (SharedString, u32);
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    fn load((path, side): Self::Source, cx: &mut App) -> impl Future<Output = Self::Output> + Send + 'static {
        let svg = cx.svg_renderer();
        async move { decode(&std::fs::read(path.as_ref())?, side, &svg) }
    }
}

fn device_side(logical: Pixels, scale: f32) -> u32 {
    (f32::from(logical) * scale).ceil().max(1.0) as u32
}

fn decode(bytes: &[u8], side: u32, svg: &SvgRenderer) -> Result<Arc<RenderImage>, ImageCacheError> {
    let Ok(format) = image::guess_format(bytes) else {
        let side = DevicePixels(i32::try_from(side).unwrap_or(i32::MAX));
        return Ok(svg.render_parsed(&svg.parse_svg(bytes)?, SvgSize::Size(size(side, side)))?);
    };
    let picture = image::load_from_memory_with_format(bytes, format)?;
    Ok(Arc::new(RenderImage::new(vec![Frame::new(to_bgra(fit(picture, side)))])))
}

fn fit(picture: DynamicImage, side: u32) -> DynamicImage {
    if picture.width() <= side && picture.height() <= side {
        return picture;
    }
    picture.resize(side, side, FilterType::Triangle)
}

fn to_bgra(picture: DynamicImage) -> image::RgbaImage {
    let mut pixels = picture.into_rgba8();
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    pixels
}

fn letter(name: &str) -> String {
    let first = name.chars().find(|character| character.is_alphanumeric());
    first.map_or_else(|| "?".into(), |character| character.to_uppercase().collect())
}

fn swatch(id: &str, swatches: usize) -> usize {
    let hash = id.encode_utf16().fold(0i32, |hash, unit| hash.wrapping_mul(31).wrapping_add(i32::from(unit)));
    hash.unsigned_abs() as usize % swatches.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    #[test]
    fn a_missing_icon_shows_the_name_s_first_letter_or_digit() {
        assert_eq!(letter("  firefox"), "F");
        assert_eq!(letter("1Password"), "1");
        assert_eq!(letter("ßeta"), "SS");
        assert_eq!(letter("***"), "?");
    }

    #[test]
    fn an_app_keeps_its_swatch() {
        assert_eq!(swatch("a", 8), 1);
        assert_eq!(swatch("firefox.desktop", 8), 7);
        assert_eq!(swatch("", 8), 0);
    }

    #[test]
    fn a_big_icon_is_kept_at_the_size_it_is_drawn() {
        let big = DynamicImage::new_rgba8(512, 256);
        let fitted = fit(big, device_side(px(30.0), 1.25));
        assert_eq!((fitted.width(), fitted.height()), (38, 19));
        let small = fit(DynamicImage::new_rgba8(16, 16), 38);
        assert_eq!((small.width(), small.height()), (16, 16));
    }

    #[test]
    fn a_picture_turns_to_the_order_the_renderer_reads() {
        let mut red = image::RgbaImage::new(1, 1);
        red.put_pixel(0, 0, image::Rgba([255, 0, 0, 128]));
        assert_eq!(to_bgra(DynamicImage::ImageRgba8(red)).get_pixel(0, 0).0, [0, 0, 255, 128]);
    }

    #[test]
    fn an_svg_and_a_png_are_both_decoded_at_the_drawn_side_in_renderer_order() {
        let svg = SvgRenderer::new(Arc::new(()));
        let square = br#"<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512"><rect width="512" height="512" fill="red"/></svg>"#;
        let mut png = Vec::new();
        DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(512, 512, image::Rgba([255, 0, 0, 255])))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        for bytes in [&square[..], &png[..]] {
            let picture = decode(bytes, 38, &svg).unwrap();
            assert_eq!(picture.size(0), size(DevicePixels(38), DevicePixels(38)));
            assert_eq!(&picture.as_bytes(0).unwrap()[..4], &[0, 0, 255, 255]);
        }
    }

    #[test]
    fn only_a_path_is_an_icon() {
        assert!(AppIcon::new("a", "A").icon(Some("/usr/share/icons/a.png")).icon.is_some());
        assert!(AppIcon::new("a", "A").icon(Some("firefox")).icon.is_none());
        assert!(AppIcon::new("a", "A").icon(None::<String>).icon.is_none());
    }
}
