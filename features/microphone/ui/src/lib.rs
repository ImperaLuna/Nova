//! What the microphone feature draws, with GPUI, in quick settings: the
//! tile that mutes the mic (`quick-settings.tiles` slot `microphone.tile`
//! of `features/microphone/feature.toml`) and the mic's slider, only while
//! an app records from it, named after those apps
//! (`quick-settings.sliders` slot `microphone.slider`).
//! `MicrophoneTileSlot.qml` and `MicrophoneSliderSlot.qml` in
//! `ui/quick_settings/slots` in today's shell.
//!
//! The feature has no work of its own in novad: both follow the sound
//! feature's `sound` topic, the input in use and who records from it, and
//! change it with the sound feature's commands.

use gpui::prelude::FluentBuilder as _;
use gpui::{App, AppContext, Context, Entity, IntoElement, Render, Subscription, Window};
use nova_look::{Glyph, Icon, Slider, Theme, Tile};
use nova_sound_api::{Command, Device, Event, Kind, Sound, TOPIC};
use nova_stage::{Part, Shown};
use nova_ui_client::{Daemon, Live};

/// The view ids the manifest's slots name.
const TILE: &str = "microphone.tile";
const SLIDER: &str = "microphone.slider";
/// The slider's fill: the second accent at this opacity
/// (`micFillOpacity`).
const FILL_OPACITY: f32 = 0.34;

/// Registers the tile and the slider with the stage.
pub fn register(cx: &mut App) {
    nova_stage::part(TILE, cx, |_, cx| cx.new(Mic::new).into());
    nova_stage::part(SLIDER, cx, |_, cx| {
        let shown = Shown::new(false);
        let slider = cx.new(|cx| Mic::new(cx).showing(shown.clone()));
        Part::new(slider).shown_while(shown)
    });
}

/// The input in use, from the sound feature. A tile, or with
/// [`Mic::showing`] a slider that shows only while an app records.
pub struct Mic {
    sound: Entity<Live<Sound>>,
    slider: Option<Shown>,
    _followed: Subscription,
}

impl Mic {
    fn new(cx: &mut Context<Self>) -> Self {
        let sound = Live::follow(TOPIC, cx, |Event::Sound(sound)| Some(sound));
        let followed = cx.observe(&sound, |mic, _, cx| {
            if let Some(shown) = mic.slider.clone() {
                // The drawer redraws when this shows or hides the slider.
                shown.set(!recorders(mic.sound.read(cx).get()).is_empty(), cx);
            }
            cx.notify();
        });
        Self { sound, slider: None, _followed: followed }
    }

    /// The slider, its `shown` set while an app records.
    fn showing(mut self, shown: Shown) -> Self {
        self.slider = Some(shown);
        self
    }
}

/// The input in use, while the sound server answers.
fn input(sound: Option<&Sound>) -> Option<&Device> {
    sound.filter(|sound| sound.available).and_then(|sound| sound.inputs.iter().find(|device| device.current))
}

fn recorders(sound: Option<&Sound>) -> &[String] {
    sound.filter(|sound| sound.available).map_or(&[], |sound| sound.recorders.as_slice())
}

/// "Discord is using the mic", "Discord and OBS are using the mic", or ""
/// when no app records (`quick_settings.js` `recordingText`).
fn recording_text(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => format!("{one} is using the mic"),
        [rest @ .., last] => format!("{} and {last} are using the mic", rest.join(", ")),
    }
}

fn mute(device: &Device, cx: &mut App) {
    Daemon::send(Command::SetSoundMuted { of: Kind::Input, id: device.id, muted: !device.muted }, cx);
}

impl Render for Mic {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sound = self.sound.read(cx).get();
        let device = input(sound).cloned();
        let muted = device.as_ref().is_some_and(|device| device.muted);
        let glyph = if muted { Glyph::MIC_OFF } else { Glyph::MIC };
        if self.slider.is_none() {
            // Lit while muted.
            return Tile::new("microphone", glyph, "Microphone")
                .on(muted)
                .available(device.is_some())
                .when_some(device, |tile, device| tile.on_toggle(move |_, _, cx| mute(&device, cx)))
                .into_any_element();
        }
        let theme = Theme::of(cx);
        let label = recording_text(recorders(sound));
        let volume = device.as_ref().map_or(0.0, |device| device.volume);
        Slider::new("microphone", Icon::new(glyph), volume, label)
            .colors(theme.secondary, theme.secondary.alpha(FILL_OPACITY))
            .muted(muted)
            .available(device.is_some())
            .when_some(device, |slider, device| {
                let (id, also) = (device.id, device.clone());
                slider
                    .on_move(move |volume, _, cx| {
                        Daemon::send(Command::SetSoundVolume { of: Kind::Input, id, volume }, cx);
                    })
                    .on_icon(move |_, _, cx| mute(&also, cx))
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slider_names_who_records() {
        let names = |list: &[&str]| list.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
        assert_eq!(recording_text(&[]), "");
        assert_eq!(recording_text(&names(&["Discord"])), "Discord is using the mic");
        assert_eq!(recording_text(&names(&["Discord", "OBS"])), "Discord and OBS are using the mic");
        assert_eq!(recording_text(&names(&["A", "B", "C"])), "A, B and C are using the mic");
    }
}
