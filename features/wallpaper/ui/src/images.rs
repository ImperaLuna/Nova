use gpui::{App, Context, DevicePixels, RenderImage, Size, Task, Window};
use image::imageops::FilterType;
use image::{DynamicImage, Frame, ImageReader};
use std::collections::HashMap;
use std::sync::Arc;

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
    Loading { _decoding: Task<()> },
    Ready(Arc<RenderImage>),
    Failed,
}

pub struct Pictures {
    keeps: usize,
    found: HashMap<Wanted, Picture>,
    order: Vec<Wanted>,
    dropped: Vec<Arc<RenderImage>>,
}

impl Pictures {
    pub fn new(keeps: usize) -> Self {
        Self { keeps, found: HashMap::new(), order: Vec::new(), dropped: Vec::new() }
    }

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
        let Some(slot) = self.found.get_mut(wanted) else { return };
        *slot = picture.map_or(Picture::Failed, Picture::Ready);
        cx.notify();
    }

    pub fn clear(&mut self) {
        self.order.clear();
        for (_, picture) in self.found.drain() {
            if let Picture::Ready(picture) = picture {
                self.dropped.push(picture);
            }
        }
    }

    pub fn release(&mut self, mut window: Option<&mut Window>, cx: &mut App) {
        for picture in self.dropped.drain(..) {
            cx.drop_image(picture, window.as_deref_mut());
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
            Ok(Some(picture)) => Some(Arc::new(picture)),
            Ok(None) => None,
            Err(problem) => {
                eprintln!("WARN nova-ui: can't show {}: {problem}", wanted.path);
                None
            }
        };
        let _ = pictures.update(cx, |pictures, cx| pictures.arrived(&wanted, picture, cx));
    })
}

fn decode(wanted: &Wanted) -> Result<Option<RenderImage>, String> {
    if std::fs::metadata(&wanted.path).is_ok_and(|metadata| metadata.is_dir()) {
        return Ok(None);
    }
    let reader = ImageReader::open(&wanted.path).map_err(|error| error.to_string())?;
    let reader = reader.with_guessed_format().map_err(|error| error.to_string())?;
    let picture: DynamicImage = reader.decode().map_err(|error| error.to_string())?;
    let mut filled = picture.resize_to_fill(wanted.width, wanted.height, FilterType::Triangle).into_rgba8();
    for pixel in filled.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(Some(RenderImage::new(vec![Frame::new(filled)])))
}
