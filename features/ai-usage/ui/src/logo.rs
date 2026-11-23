//! The tools' own marks, filled in one color: the brands' 24x24 glyphs from
//! LobeHub's icon set (@lobehub/icons-static-svg, MIT), Anthropic's for
//! Claude and Codex's own, as `ui/ai_usage/ProviderLogo.qml` draws them
//! (a test holds the paths equal).
//!
//! The look kit's icons are line drawings; these are solid shapes with
//! holes (even-odd), so each is a small SVG picture of its own, made in the
//! color asked for. GPUI keeps the drawn picture per document.

use gpui::{Hsla, Image, ImageFormat, IntoElement, Pixels, Rgba, Styled, img};
use nova_ai_usage_api::AiProvider;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

const CLAUDE: &str = "M13.827 3.52h3.603L24 20h-3.603l-6.57-16.48zm-7.258 0h3.767L16.906 20h-3.674l-1.343-3.461H5.017l-1.344 3.46H0L6.57 3.522zm4.132 9.959L8.453 7.687 6.205 13.48H10.7z";
const CODEX: &str = "M8.086.457a6.105 6.105 0 013.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 00.107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 01-.18 1.631.167.167 0 00.04.155 5.982 5.982 0 011.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 01-2.934 1.851.162.162 0 00-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 00-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 01-2.595-.622 6.058 6.058 0 01-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 01-.495-1.283 6.11 6.11 0 01-.017-3.064.166.166 0 00.008-.074.115.115 0 00-.037-.064 5.958 5.958 0 01-1.38-2.202 5.196 5.196 0 01-.333-1.589 6.915 6.915 0 01.188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 00.087-.087A6.016 6.016 0 015.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 00-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 001.46.864l1.94-3.272a.849.849 0 00.007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 000 1.695h4.848a.849.849 0 000-1.696h-4.848z";

fn path(provider: AiProvider) -> &'static str {
    match provider {
        AiProvider::Claude => CLAUDE,
        AiProvider::Codex => CODEX,
    }
}

/// The SVG document of `provider`'s mark in `color`.
fn document(provider: AiProvider, color: Hsla) -> String {
    let Rgba { r, g, b, a } = color.into();
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path fill="rgb({},{},{})" fill-opacity="{a}" fill-rule="evenodd" d="{}"/></svg>"#,
        channel(r),
        channel(g),
        channel(b),
        path(provider)
    )
}

thread_local! {
    /// Each mark made so far, by tool and color: a spinning pill draws its
    /// logo every frame.
    static MADE: RefCell<HashMap<(&'static str, u32), Arc<Image>>> = RefCell::new(HashMap::new());
}

/// `provider`'s mark, `size` square, in `color`.
pub fn logo(provider: AiProvider, size: Pixels, color: Hsla) -> impl IntoElement {
    let key = (path(provider), u32::from(Rgba::from(color)));
    let picture = MADE.with_borrow_mut(|made| {
        made.entry(key)
            .or_insert_with(|| Arc::new(Image::from_bytes(ImageFormat::Svg, document(provider, color).into_bytes())))
            .clone()
    });
    img(picture).size(size).flex_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_is_drawn_in_the_color_asked_for() {
        let white = gpui::white();
        let svg = document(AiProvider::Claude, white);
        assert!(svg.contains(r#"fill="rgb(255,255,255)""#), "{svg}");
        assert!(svg.contains(r#"fill-rule="evenodd""#), "{svg}");
        assert!(svg.contains(CLAUDE), "{svg}");
    }
}
