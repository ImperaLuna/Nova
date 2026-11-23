//! The folder's images as the carousel draws them: each decoded off the
//! main thread at the size it is drawn, never the file's, and cut to fill
//! that box as awww fills a screen. A card costs a picture the size of a
//! card, a preview one the size of the screen. Today's QML does the same
//! with `Image.sourceSize` and `PreserveAspectCrop`.
//!
//! [`Pictures`] keeps the few it was last asked for and lets the others go,
//! from the GPU's atlas too, so stepping through a big folder holds only
//! the cards around the middle.

use gpui::{App, Context, DevicePixels, RenderImage, Size, Task, Window};
use image::imageops::FilterType;
use image::{DynamicImage, Frame, ImageReader};
use std::collections::HashMap;
use std::sync::Arc;

/// How an image was asked for: its file, and the box in pixels it fills.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Wanted {
    pub path: String,
    pub width: u32,
    pub height: u32,
}

impl Wanted {
    pub fn new(path: &str, pixels: Size<DevicePixels>) -> Self {
        let side = |length: DevicePixels| u32::try_from(length.0).unwrap_or(0).max(1);
        Self { path: path.to_owned(), width: side(pixels.width), height: side(pixels.height) }
    }
}

enum Picture {
    /// Decoding; dropping the task stops it.
    Loading {
        _decoding: Task<()>,
    },
    Ready(Arc<RenderImage>),
    Failed,
}

/// Decoded pictures, kept as an entity: it notifies when one arrives, so
/// the views drawing it observe it.
pub struct Pictures {
    /// How many it keeps once they are not asked for.
    keeps: usize,
    found: HashMap<Wanted, Picture>,
    /// Least recently asked for first.
    order: Vec<Wanted>,
    /// Let go since the last frame: still in the GPU's atlas.
    dropped: Vec<Arc<RenderImage>>,
}

impl Pictures {
    pub fn new(keeps: usize) -> Self {
        Self { keeps, found: HashMap::new(), order: Vec::new(), dropped: Vec::new() }
    }

    /// The picture `wanted` asks for once it is decoded; until then `None`,
    /// and the decoding starts. The entity notifies when it is ready.
    pub fn get(&mut self, wanted: &Wanted, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        self.touch(wanted);
        if !self.found.contains_key(wanted) {
            let task = load(wanted.clone(), cx);
            self.found.insert(wanted.clone(), Picture::Loading { _decoding: task });
        }
        match self.found.get(wanted) {
            Some(Picture::Ready(picture)) => Some(picture.clone()),
            _ => None,
        }
    }

    fn touch(&mut self, wanted: &Wanted) {
        self.order.retain(|each| each != wanted);
        self.order.push(wanted.clone());
        while self.order.len() > self.keeps {
            let oldest = self.order.remove(0);
            if let Some(Picture::Ready(picture)) = self.found.remove(&oldest) {
                self.dropped.push(picture);
            }
        }
    }

    fn arrived(&mut self, wanted: &Wanted, picture: Option<Arc<RenderImage>>, cx: &mut Context<Self>) {
        // Let go while it was decoding.
        let Some(slot) = self.found.get_mut(wanted) else { return };
        *slot = picture.map_or(Picture::Failed, Picture::Ready);
        cx.notify();
    }

    /// Lets every picture go: the carousel closed.
    pub fn clear(&mut self) {
        self.order.clear();
        for (_, picture) in self.found.drain() {
            if let Picture::Ready(picture) = picture {
                self.dropped.push(picture);
            }
        }
    }

    /// Takes what was let go out of the GPU's atlas, from `window` (being
    /// drawn) and every other.
    pub fn release(&mut self, window: &mut Window, cx: &mut App) {
        for picture in self.dropped.drain(..) {
            cx.drop_image(picture, Some(window));
        }
    }
}

fn load(wanted: Wanted, cx: &mut Context<Pictures>) -> Task<()> {
    let decoding = {
        let wanted = wanted.clone();
        cx.background_executor().spawn(async move { decode(&wanted) })
    };
    cx.spawn(async move |pictures, cx| {
        let picture = match decoding.await {
            Ok(picture) => Some(Arc::new(picture)),
            Err(problem) => {
                eprintln!("WARN nova-ui: can't show {}: {problem}", wanted.path);
                None
            }
        };
        let _ = pictures.update(cx, |pictures, cx| pictures.arrived(&wanted, picture, cx));
    })
}

/// Reads the file and cuts it to fill the box.
fn decode(wanted: &Wanted) -> Result<RenderImage, String> {
    let reader = ImageReader::open(&wanted.path).map_err(|error| error.to_string())?;
    let reader = reader.with_guessed_format().map_err(|error| error.to_string())?;
    let picture: DynamicImage = reader.decode().map_err(|error| error.to_string())?;
    let mut filled = picture.resize_to_fill(wanted.width, wanted.height, FilterType::Triangle).into_rgba8();
    // GPUI draws BGRA.
    for pixel in filled.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(RenderImage::new(vec![Frame::new(filled)]))
}
