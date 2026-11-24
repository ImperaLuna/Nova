use crate::reading::{BLUEZ_INPUT, BLUEZ_OUTPUT, app_streams, at_level, level};
use crate::server::Listing;
use nova_sound_api::{Command, Kind, VOLUME_MAX, VOLUME_STEP};
use nova_volume_osd_api::VolumeKey;
use pulseaudio::protocol::{
    Command as Request, SetDeviceMuteParams, SetDeviceVolumeParams, SetStreamMuteParams, SetStreamVolumeParams,
    SinkInfo, SinkInputInfo, SourceInfo,
};

pub fn requests(listing: &Listing, command: &Command) -> Result<Vec<Request>, String> {
    match *command {
        Command::SetSoundVolume { of, id, volume } => set_volume(listing, of, id, volume, true),
        Command::SetSoundLevel { of, id, volume } => set_volume(listing, of, id, volume, false),
        Command::SetSoundMuted { of: Kind::Output, id, muted } => {
            Ok(vec![Request::SetSinkMute(device_mute(output(listing, id)?.index, muted))])
        }
        Command::SetSoundMuted { of: Kind::Input, id, muted } => {
            Ok(vec![Request::SetSourceMute(device_mute(input(listing, id)?.index, muted))])
        }
        Command::SetSoundMuted { of: Kind::App, id, muted } => Ok(app(listing, id)?
            .into_iter()
            .map(|stream| Request::SetSinkInputMute(SetStreamMuteParams { index: stream.index, mute: muted }))
            .collect()),
        Command::UseSoundOutput { id } => Ok(vec![Request::SetDefaultSink(output(listing, id)?.name.clone())]),
        Command::UseSoundInput { id } => Ok(vec![Request::SetDefaultSource(input(listing, id)?.name.clone())]),
    }
}

pub fn press(listing: &Listing, key: VolumeKey) -> Result<Vec<Request>, String> {
    let default = listing.server.default_sink_name.as_deref();
    let Some(sink) = listing.sinks.iter().find(|sink| Some(sink.name.as_c_str()) == default) else {
        return Err("there is no output in use".into());
    };
    let direction = match key {
        VolumeKey::Mute => return Ok(vec![Request::SetSinkMute(device_mute(sink.index, !sink.muted))]),
        VolumeKey::Up => 1.0,
        VolumeKey::Down => -1.0,
    };
    let now = level(&sink.cvolume);
    let next = stepped(now, direction, VOLUME_STEP);
    if next == now {
        return Ok(Vec::new());
    }
    Ok(vec![Request::SetSinkVolume(device_volume(sink.index, at_level(&sink.cvolume, next)))])
}

pub fn apply(listing: &mut Listing, requests: &[Request]) {
    for request in requests {
        match request {
            Request::SetSinkVolume(params) => {
                if let Some(sink) = listing.sinks.iter_mut().find(|sink| Some(sink.index) == params.device_index) {
                    sink.cvolume = params.volume;
                }
            }
            Request::SetSinkMute(params) => {
                if let Some(sink) = listing.sinks.iter_mut().find(|sink| Some(sink.index) == params.device_index) {
                    sink.muted = params.mute;
                }
            }
            _ => {}
        }
    }
}

pub fn stepped(volume: f32, direction: f32, step: f32) -> f32 {
    if direction > 0.0 && volume >= VOLUME_MAX {
        return volume;
    }
    let from = volume.min(VOLUME_MAX);
    let next = ((from + direction * step) / step).round() * step;
    (next.clamp(0.0, VOLUME_MAX) * 100.0).round() / 100.0
}

pub fn arrived_bluetooth(before: Option<&Listing>, now: &Listing) -> Vec<Request> {
    let Some(before) = before else { return Vec::new() };
    let new_output = now.sinks.iter().find(|sink| {
        sink.name.to_bytes().starts_with(BLUEZ_OUTPUT.as_bytes())
            && !before.sinks.iter().any(|known| known.index == sink.index)
    });
    let new_input = now.sources.iter().find(|source| {
        source.name.to_bytes().starts_with(BLUEZ_INPUT.as_bytes())
            && !before.sources.iter().any(|known| known.index == source.index)
    });
    let outputs = new_output.map(|sink| Request::SetDefaultSink(sink.name.clone()));
    let inputs = new_input.map(|source| Request::SetDefaultSource(source.name.clone()));
    outputs.into_iter().chain(inputs).collect()
}

fn set_volume(listing: &Listing, of: Kind, id: u32, volume: f32, unmutes: bool) -> Result<Vec<Request>, String> {
    if !(0.0..=VOLUME_MAX).contains(&volume) {
        return Err(format!("a volume is 0 to {VOLUME_MAX}, not {volume}"));
    }
    let mut requests = Vec::new();
    match of {
        Kind::Output => {
            let sink = output(listing, id)?;
            requests.push(Request::SetSinkVolume(device_volume(sink.index, at_level(&sink.cvolume, volume))));
            if unmutes && sink.muted {
                requests.push(Request::SetSinkMute(device_mute(sink.index, false)));
            }
        }
        Kind::Input => {
            let source = input(listing, id)?;
            requests.push(Request::SetSourceVolume(device_volume(source.index, at_level(&source.cvolume, volume))));
            if unmutes && source.muted {
                requests.push(Request::SetSourceMute(device_mute(source.index, false)));
            }
        }
        Kind::App => {
            for stream in app(listing, id)? {
                if stream.has_volume && stream.volume_writable {
                    let volume = at_level(&stream.cvolume, volume);
                    requests.push(Request::SetSinkInputVolume(SetStreamVolumeParams { index: stream.index, volume }));
                }
                if unmutes && stream.muted {
                    requests.push(Request::SetSinkInputMute(SetStreamMuteParams { index: stream.index, mute: false }));
                }
            }
        }
    }
    Ok(requests)
}

fn output(listing: &Listing, id: u32) -> Result<&SinkInfo, String> {
    listing.sinks.iter().find(|sink| sink.index == id).ok_or_else(|| format!("there is no output {id}"))
}

fn input(listing: &Listing, id: u32) -> Result<&SourceInfo, String> {
    let inputs = listing.sources.iter().filter(|source| source.monitor_of_sink_index.is_none());
    inputs.into_iter().find(|source| source.index == id).ok_or_else(|| format!("there is no input {id}"))
}

fn app(listing: &Listing, id: u32) -> Result<Vec<&SinkInputInfo>, String> {
    let ids = app_streams(&listing.sink_inputs, id);
    if ids.is_empty() {
        return Err(format!("no app plays as {id}"));
    }
    Ok(listing.sink_inputs.iter().filter(|stream| ids.contains(&stream.index)).collect())
}

fn device_volume(index: u32, volume: pulseaudio::protocol::ChannelVolume) -> SetDeviceVolumeParams {
    SetDeviceVolumeParams { device_index: Some(index), device_name: None, volume }
}

fn device_mute(index: u32, mute: bool) -> SetDeviceMuteParams {
    SetDeviceMuteParams { device_index: Some(index), device_name: None, mute }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_lands_on_the_grid_and_stops_at_full() {
        let up = |volume| stepped(volume, 1.0, VOLUME_STEP);
        let down = |volume| stepped(volume, -1.0, VOLUME_STEP);
        assert_eq!(up(0.4), 0.45);
        assert_eq!(up(0.42), 0.45, "off the grid, it lands on it");
        assert_eq!(down(0.42), 0.35);
        assert_eq!(up(0.98), 1.0);
        assert_eq!(up(1.0), 1.0);
        assert_eq!(up(1.3), 1.3, "above 100% (set by another app), up leaves it");
        assert_eq!(down(1.3), 0.95, "down steps from 100%");
        assert_eq!(down(0.02), 0.0);
        assert_eq!(down(0.0), 0.0);
        assert_eq!(up(0.399_99), 0.45);
    }
}
