//! `sound.slider`: the volume of the output in use, in quick settings; its
//! arrow opens the Sound page.

use crate::logic::{self, output_kind};
use crate::{PAGE, QUICK_SETTINGS};
use gpui::{Context, Entity, IntoElement, Render, Subscription, Window};
use nova_look::{Slider, VolumeGlyph};
use nova_sound_api::{Command, Event, Kind, Sound, TOPIC};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Daemon, Live};

pub struct SoundSlider {
    screen: Screen,
    sound: Entity<Live<Sound>>,
    _followed: Subscription,
}

impl SoundSlider {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let sound = Live::follow(TOPIC, cx, |Event::Sound(sound)| Some(sound));
        let followed = cx.observe(&sound, |_, _, cx| cx.notify());
        Self { screen: screen.clone(), sound, _followed: followed }
    }
}

impl Render for SoundSlider {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let output = logic::output(self.sound.read(cx).get()).cloned();
        let glyph = VolumeGlyph::pick(
            output.as_ref().map(|output| output_kind(output.kind)),
            output.as_ref().is_some_and(|output| output.muted),
            output.as_ref().map_or(0.0, |output| output.volume),
        );
        let screen = self.screen.id;
        let slider = Slider::new(
            "sound",
            glyph.icon(),
            output.as_ref().map_or(0.0, |output| output.volume),
            output.as_ref().map_or_else(|| "No output".to_owned(), |output| output.name.clone()),
        )
        .available(output.is_some())
        .muted(output.as_ref().is_some_and(|output| output.muted))
        .on_open(move |_, _, cx| Stage::show_page(QUICK_SETTINGS, screen, Some(PAGE), cx));
        let Some(output) = output else { return slider };
        let (id, muted) = (output.id, output.muted);
        slider
            .on_move(move |volume, _, cx| Daemon::send(Command::SetSoundVolume { of: Kind::Output, id, volume }, cx))
            .on_icon(move |_, _, cx| Daemon::send(Command::SetSoundMuted { of: Kind::Output, id, muted: !muted }, cx))
    }
}
