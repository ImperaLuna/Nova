use crate::server::Listing;
use nova_sound_api::{App, Device, DeviceKind, Link, Sound};
use pulseaudio::protocol::{ChannelVolume, Props, SinkInputInfo, Volume};
use std::ffi::CStr;

const APPLICATION_NAME: &CStr = c"application.name";
const DEVICE_API: &CStr = c"device.api";
const DEVICE_BUS: &CStr = c"device.bus";
const DEVICE_PROFILE_NAME: &CStr = c"device.profile.name";
const NODE_DESCRIPTION: &CStr = c"node.description";
const NODE_NAME: &CStr = c"node.name";
const NODE_NICK: &CStr = c"node.nick";
const SAYS_WHAT_IT_IS: &[&CStr] =
    &[c"device.form-factor", c"device.form_factor", c"device.icon-name", c"device.icon_name", c"node.icon-name"];

const BLUEZ_API: &str = "bluez5";
pub const BLUEZ_OUTPUT: &str = "bluez_output.";
pub const BLUEZ_INPUT: &str = "bluez_input.";
const BLUEZ_NAMES: &[&str] = &[BLUEZ_OUTPUT, BLUEZ_INPUT];
const USB_BUS: &str = "usb";
const USB_NAMES: &[&str] = &["alsa_output.usb-", "alsa_input.usb-"];
const HEAD_WORN: &[&str] = &["headphone", "headset"];
const DISPLAY_PORTS: &[&str] = &["hdmi", "displayport"];

const FULL: f32 = 1.0;

pub fn sound(listing: &Listing) -> Sound {
    let default_output = listing.server.default_sink_name.as_deref();
    let default_input = listing.server.default_source_name.as_deref();
    let outputs = listing.sinks.iter().map(|sink| {
        let named = Named { name: &sink.name, description: sink.description.as_deref(), props: &sink.props };
        named.device(sink.index, &sink.cvolume, sink.muted, default_output)
    });
    let microphones = listing.sources.iter().filter(|source| source.monitor_of_sink_index.is_none());
    let inputs = microphones.clone().map(|source| {
        let named = Named { name: &source.name, description: source.description.as_deref(), props: &source.props };
        named.device(source.index, &source.cvolume, source.muted, default_input)
    });
    let current_input = microphones.clone().find(|source| Some(source.name.as_c_str()) == default_input);
    let recording = listing
        .source_outputs
        .iter()
        .filter(|stream| current_input.is_some_and(|input| stream.source_index == input.index));
    let mut recorders: Vec<String> = Vec::new();
    for stream in recording {
        let name = stream_name(&stream.props, &stream.name);
        if !name.is_empty() && !recorders.contains(&name) {
            recorders.push(name);
        }
    }
    Sound {
        available: true,
        outputs: outputs.collect(),
        inputs: inputs.collect(),
        apps: apps(&listing.sink_inputs),
        recorders,
    }
}

struct Named<'a> {
    name: &'a CStr,
    description: Option<&'a CStr>,
    props: &'a Props,
}

impl Named<'_> {
    fn device(&self, id: u32, volume: &ChannelVolume, muted: bool, default: Option<&CStr>) -> Device {
        let node_name = self.name.to_string_lossy();
        let description = self.description.map(CStr::to_string_lossy).unwrap_or_default();
        let name = [prop(self.props, NODE_NICK).unwrap_or_default(), &description, &node_name]
            .into_iter()
            .find(|name| !name.is_empty())
            .unwrap_or_default()
            .to_owned();
        Device {
            id,
            name,
            kind: self.kind(&node_name, &description),
            link: self.link(&node_name),
            volume: level(volume),
            muted,
            current: default == Some(self.name),
        }
    }

    fn kind(&self, node_name: &str, description: &str) -> DeviceKind {
        let says = |keys: &[&CStr], words: &[&str]| {
            keys.iter().filter_map(|key| prop(self.props, key)).any(|value| contains_any(value, words))
        };
        if says(SAYS_WHAT_IT_IS, HEAD_WORN) {
            return DeviceKind::Headphones;
        }
        if self.is_bluetooth(node_name) {
            return DeviceKind::Bluetooth;
        }
        let profile = prop(self.props, DEVICE_PROFILE_NAME).unwrap_or_default();
        if [node_name, description, profile].iter().any(|text| contains_any(&text.to_lowercase(), DISPLAY_PORTS)) {
            return DeviceKind::Display;
        }
        DeviceKind::Speakers
    }

    fn link(&self, node_name: &str) -> Option<Link> {
        if self.is_bluetooth(node_name) {
            Some(Link::Bluetooth)
        } else if prop(self.props, DEVICE_BUS) == Some(USB_BUS) || starts_with_any(node_name, USB_NAMES) {
            Some(Link::Usb)
        } else {
            None
        }
    }

    fn is_bluetooth(&self, node_name: &str) -> bool {
        prop(self.props, DEVICE_API) == Some(BLUEZ_API) || starts_with_any(node_name, BLUEZ_NAMES)
    }
}

fn apps(streams: &[SinkInputInfo]) -> Vec<App> {
    let mut apps: Vec<(App, Option<f32>)> = Vec::new();
    for stream in streams {
        let name = stream_name(&stream.props, &stream.name);
        let volume = stream.has_volume.then(|| level(&stream.cvolume));
        match apps.iter_mut().find(|(app, _)| app.name == name) {
            Some((app, loudest)) => {
                *loudest = match (*loudest, volume) {
                    (Some(one), Some(other)) => Some(one.max(other)),
                    (one, other) => one.or(other),
                };
                app.muted = app.muted && stream.muted;
            }
            None => apps.push((App { id: stream.index, name, volume: 0.0, muted: stream.muted }, volume)),
        }
    }
    apps.into_iter().map(|(app, loudest)| App { volume: loudest.unwrap_or(FULL), ..app }).collect()
}

pub fn app_streams(streams: &[SinkInputInfo], id: u32) -> Vec<u32> {
    let name_of = |stream: &SinkInputInfo| stream_name(&stream.props, &stream.name);
    let Some(app) = streams.iter().find(|stream| stream.index == id).map(name_of) else {
        return Vec::new();
    };
    streams.iter().filter(|stream| name_of(stream) == app).map(|stream| stream.index).collect()
}

fn stream_name(props: &Props, name: &CStr) -> String {
    [APPLICATION_NAME, NODE_DESCRIPTION, NODE_NAME]
        .into_iter()
        .filter_map(|key| prop(props, key))
        .find(|value| !value.is_empty())
        .map_or_else(|| name.to_string_lossy().into_owned(), str::to_owned)
}

pub fn level(channels: &ChannelVolume) -> f32 {
    let channels = channels.channels();
    if channels.is_empty() {
        return 0.0;
    }
    let sum: f64 = channels.iter().map(|volume| f64::from(volume.as_u32())).sum();
    (sum / channels.len() as f64 / f64::from(Volume::NORM.as_u32())) as f32
}

pub fn at_level(channels: &ChannelVolume, volume: f32) -> ChannelVolume {
    let norm = f64::from(Volume::NORM.as_u32());
    let now = f64::from(level(channels));
    let mut moved = ChannelVolume::empty();
    for channel in channels.channels() {
        let share = if now > 0.0 { f64::from(channel.as_u32()) / norm / now } else { 1.0 };
        moved.push(Volume::from_u32_clamped((share * f64::from(volume) * norm).round() as u32));
    }
    moved
}

fn prop<'a>(props: &'a Props, key: &CStr) -> Option<&'a str> {
    let bytes = props.get_bytes(key)?;
    std::str::from_utf8(bytes.strip_suffix(&[0]).unwrap_or(bytes)).ok()
}

fn contains_any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|word| text.contains(word))
}

fn starts_with_any(text: &str, starts: &[&str]) -> bool {
    starts.iter().any(|start| text.starts_with(start))
}
