//! `sound.page`: the Sound page in quick settings. The device in use with
//! its volume on top, then tabs: each app's own volume, and every output
//! and input to switch to. `SoundPage.qml` in today's shell.
//!
//! Apps come with a name only; their icons are asked of novad's launcher
//! (`app_icon`) once per name while the page shows, and drawn as the
//! answers come. Until then, and without one, an app shows its letter.

use crate::logic::{self, input, kind_glyph, link_label, output, output_kind, subtitle};
use crate::{PAGE, QUICK_SETTINGS};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, div,
};
use nova_launcher_api::Command as LauncherCommand;
use nova_look::tokens::{
    FONT_SIZE, FONT_SIZE_SMALL, GAP, LABEL_NUDGE, LINE_HEIGHT, PAD, QUICK_SETTINGS_GAP, RADIUS_CARD, ROW_INSET,
    WEIGHT_TITLE,
};
use nova_look::{Glyph, Icon, ListRow, LookStyled, PageHeader, Segments, Slider, Theme, VolumeGlyph};
use nova_sound_api::{Command, Event, Kind, Sound, TOPIC};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Daemon, Live};
use std::collections::HashMap;

/// The tabs, the apps first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Apps,
    Output,
    Input,
}

impl Tab {
    const ALL: [(Tab, &'static str, &'static str); 3] =
        [(Tab::Apps, "apps", "Apps"), (Tab::Output, "output", "Output"), (Tab::Input, "input", "Input")];

    fn key(self) -> &'static str {
        Self::ALL.iter().find(|(tab, ..)| *tab == self).map_or("apps", |(_, key, _)| key)
    }

    fn from_key(key: &str) -> Self {
        Self::ALL.iter().find(|(_, each, _)| *each == key).map_or(Self::Apps, |(tab, ..)| *tab)
    }
}

pub struct SoundPage {
    screen: Screen,
    sound: Entity<Live<Sound>>,
    tab: Tab,
    /// Whether quick settings shows this page on its screen.
    shown: bool,
    /// Each app's icon by its name, once novad answered (`None` for none).
    icons: HashMap<String, Option<SharedString>>,
    asking: Vec<Task<()>>,
    _followed: Vec<Subscription>,
}

impl SoundPage {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let sound = Live::follow(TOPIC, cx, |Event::Sound(sound)| Some(sound));
        let mut followed = vec![cx.observe(&sound, |page, _, cx| {
            page.ask_icons(cx);
            cx.notify();
        })];
        followed.extend(Stage::observe_panels(cx, |page, cx| page.follow_stage(cx)));
        Self {
            screen: screen.clone(),
            sound,
            tab: Tab::default(),
            shown: false,
            icons: HashMap::new(),
            asking: Vec::new(),
            _followed: followed,
        }
    }

    /// Shown or not: it opens on the Apps tab each time.
    fn follow_stage(&mut self, cx: &mut Context<Self>) {
        let shown = Stage::is_open(QUICK_SETTINGS, self.screen.id, cx)
            && Stage::page(QUICK_SETTINGS, self.screen.id, cx).as_deref() == Some(PAGE);
        if shown != self.shown {
            self.shown = shown;
            if shown {
                self.tab = Tab::default();
                self.ask_icons(cx);
            }
            cx.notify();
        }
    }

    /// Asks novad for the icons of the apps not asked about yet.
    fn ask_icons(&mut self, cx: &mut Context<Self>) {
        if !self.shown {
            return;
        }
        let names: Vec<String> = self
            .sound
            .read(cx)
            .get()
            .map(|sound| sound.apps.iter().map(|app| app.name.clone()).collect())
            .unwrap_or_default();
        for name in names {
            if self.icons.contains_key(&name) {
                continue;
            }
            self.icons.insert(name.clone(), None);
            let ask = LauncherCommand::AppIcon { icon_name: String::new(), program: String::new(), name: name.clone() };
            let answer = Daemon::call::<Option<String>>(ask, cx);
            self.asking.push(cx.spawn(async move |page, cx| {
                // No icon, or no launcher to ask: the letter stays.
                let Ok(Some(icon)) = answer.await else { return };
                let _ = page.update(cx, |page, cx| {
                    page.icons.insert(name, Some(icon.into()));
                    cx.notify();
                });
            }));
        }
    }

    fn pick_tab(&mut self, key: &SharedString, cx: &mut Context<Self>) {
        self.tab = Tab::from_key(key);
        cx.notify();
    }
}

impl Render for SoundPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = self.screen.id;
        let sound = self.sound.read(cx).get().cloned();
        let entity = cx.entity();
        let tabs = Segments::new(Tab::ALL.map(|(_, key, label)| (key, label)), self.tab.key())
            .on_pick(move |key, _, cx| entity.update(cx, |page, cx| page.pick_tab(key, cx)));
        let list = match self.tab {
            Tab::Apps => self.apps(sound.as_ref(), cx),
            Tab::Output | Tab::Input => self.devices(sound.as_ref(), self.tab == Tab::Input),
        };
        div()
            .size_full()
            .body_text(cx)
            .flex()
            .flex_col()
            .gap(QUICK_SETTINGS_GAP)
            .child(PageHeader::new("Sound", move |_, _, cx| Stage::show_page(QUICK_SETTINGS, screen, None, cx)))
            .child(self.in_use(sound.as_ref(), cx))
            .child(tabs)
            .child(div().id("lists").flex_1().min_h_0().overflow_y_scroll().child(list))
    }
}

impl SoundPage {
    /// The card on top: the device in use, how it's connected, and its
    /// volume; the input's on the Input tab, else the output's.
    fn in_use(&self, sound: Option<&Sound>, cx: &Context<Self>) -> impl IntoElement + use<> {
        let theme = Theme::of(cx);
        let is_input = self.tab == Tab::Input;
        let (device, devices) = match is_input {
            true => (input(sound), sound.map(|sound| sound.inputs.as_slice())),
            false => (output(sound), sound.map(|sound| sound.outputs.as_slice())),
        };
        let device = device.cloned();
        let name = match (&device, is_input) {
            (Some(device), _) => device.name.clone(),
            (None, true) => "No input".to_owned(),
            (None, false) => "No output".to_owned(),
        };
        let link = link_label(devices.and_then(logic::current).and_then(|device| device.link));
        let (volume, muted) = device.as_ref().map_or((0.0, false), |device| (device.volume, device.muted));
        let (kind, label, id) = match is_input {
            true => (Kind::Input, "Input", "input"),
            false => (Kind::Output, "Output", "output"),
        };
        let icon = match is_input {
            true => Icon::new(if muted { Glyph::MIC_OFF } else { Glyph::MIC }),
            false => VolumeGlyph::pick(device.as_ref().map(|device| output_kind(device.kind)), muted, volume).icon(),
        };
        let mut slider = Slider::new(id, icon, volume, label).available(device.is_some()).muted(muted);
        if let Some(device) = &device {
            let id = device.id;
            slider = slider
                .on_move(move |volume, _, cx| Daemon::send(Command::SetSoundVolume { of: kind, id, volume }, cx))
                .on_icon(move |_, _, cx| Daemon::send(Command::SetSoundMuted { of: kind, id, muted: !muted }, cx));
        }
        div()
            .flex_none()
            .p(PAD)
            .rounded(RADIUS_CARD)
            .bg(theme.surface_raised())
            .flex()
            .flex_col()
            .gap(ROW_INSET)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().truncate().text_size(FONT_SIZE).font_weight(WEIGHT_TITLE).child(name))
                    // A line tall even when empty, so the card keeps its height.
                    .child(
                        div()
                            .min_h(FONT_SIZE_SMALL * LINE_HEIGHT)
                            .truncate()
                            .text_size(FONT_SIZE_SMALL)
                            .text_color(theme.fg_dim())
                            .child(link),
                    ),
            )
            .child(slider)
    }

    /// Every output or input, the one in use marked; a click switches.
    fn devices(&self, sound: Option<&Sound>, is_input: bool) -> AnyElement {
        let devices = sound.filter(|sound| sound.available).map_or(&[][..], |sound| match is_input {
            true => sound.inputs.as_slice(),
            false => sound.outputs.as_slice(),
        });
        let rows = devices.iter().map(|device| {
            let glyph = if is_input { Glyph::MIC } else { kind_glyph(device.kind) };
            let id = device.id;
            let command = move || match is_input {
                true => Command::UseSoundInput { id },
                false => Command::UseSoundOutput { id },
            };
            ListRow::new(("device", device.id as usize), glyph, device.name.clone())
                .subtitle(subtitle(device, is_input))
                .status(if device.current { "In use" } else { "" })
                .selected(device.current)
                .on_click(move |_, _, cx| Daemon::send(command(), cx))
        });
        div().flex().flex_col().gap(LABEL_NUDGE * 2.0).children(rows).into_any_element()
    }

    /// Each app's own volume, or a quiet line when nothing plays.
    fn apps(&self, sound: Option<&Sound>, cx: &Context<Self>) -> AnyElement {
        let apps = sound.filter(|sound| sound.available).map_or(&[][..], |sound| sound.apps.as_slice());
        let sliders = apps.iter().enumerate().map(|(at, app)| {
            let icon = self.icons.get(&app.name).cloned().flatten();
            let (id, muted) = (app.id, app.muted);
            Slider::app(("app", at), app.name.clone(), icon, app.volume)
                .muted(muted)
                .on_move(move |volume, _, cx| Daemon::send(Command::SetSoundVolume { of: Kind::App, id, volume }, cx))
                .on_icon(move |_, _, cx| Daemon::send(Command::SetSoundMuted { of: Kind::App, id, muted: !muted }, cx))
        });
        div()
            .flex()
            .flex_col()
            .gap(GAP)
            .children(sliders)
            .when(apps.is_empty(), |list| {
                list.child(
                    div()
                        .pl(PAD)
                        .text_size(FONT_SIZE_SMALL)
                        .text_color(Theme::of(cx).fg_dim())
                        .child("Nothing playing"),
                )
            })
            .into_any_element()
    }
}
