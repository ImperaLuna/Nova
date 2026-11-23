//! What the sound feature draws, with GPUI: the volume slider in quick
//! settings (`quick-settings.sliders` slot `sound.slider` of
//! `features/sound/feature.toml`) and the Sound page its arrow opens
//! (`quick-settings.pages` slot `sound.page`). `SoundSliderSlot.qml` and
//! `SoundPage.qml` in `ui/quick_settings` in today's shell, which stay the
//! spec until it is removed.
//!
//! Both follow novad's `sound` topic and change things with the sound
//! feature's commands; what they show comes back as the next event.
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `slider`, `page`: the two views.
//! - `logic`: what `quick_settings.js` and `volume.js` worked out for them
//!   (the device in use, its icon, a row's second line), tested alone.

mod logic;
mod page;
mod slider;

use gpui::{App, AppContext};

/// The view ids the manifest's slots name.
const SLIDER: &str = "sound.slider";
const PAGE: &str = "sound.page";
/// The drawer the page is in, by its name, and the page's own name there
/// (its view id).
const QUICK_SETTINGS: &str = "quick-settings";

/// Registers the slider and the page with the stage.
pub fn register(cx: &mut App) {
    nova_stage::part(SLIDER, cx, |screen, cx| cx.new(|cx| slider::SoundSlider::new(screen, cx)).into());
    nova_stage::part(PAGE, cx, |screen, cx| cx.new(|cx| page::SoundPage::new(screen, cx)).into());
}
