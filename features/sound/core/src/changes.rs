//! What the server is told for each of the feature's commands: which of
//! its sinks, sources and streams a command's id names in the latest
//! [`Listing`], and the requests that change them. Plain functions, so the
//! service only sends what comes out.

use crate::reading::{app_streams, at_level};
use crate::server::Listing;
use nova_sound_api::{Command, Kind, VOLUME_MAX};
use pulseaudio::protocol::{
    Command as Request, SetDeviceMuteParams, SetDeviceVolumeParams, SetStreamMuteParams, SetStreamVolumeParams,
    SinkInfo, SinkInputInfo, SourceInfo,
};

/// The requests that do `command`, in order, or why it can't be done.
pub fn requests(listing: &Listing, command: &Command) -> Result<Vec<Request>, String> {
    match *command {
        Command::SetSoundVolume { of, id, volume } => {
            if !(0.0..=VOLUME_MAX).contains(&volume) {
                return Err(format!("a volume is 0 to {VOLUME_MAX}, not {volume}"));
            }
            set_volume(listing, of, id, volume)
        }
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

/// Moved by hand, so unmuted as well: moving a slider means wanting to
/// hear it.
fn set_volume(listing: &Listing, of: Kind, id: u32, volume: f32) -> Result<Vec<Request>, String> {
    let mut requests = Vec::new();
    match of {
        Kind::Output => {
            let sink = output(listing, id)?;
            requests.push(Request::SetSinkVolume(device_volume(sink.index, at_level(&sink.cvolume, volume))));
            if sink.muted {
                requests.push(Request::SetSinkMute(device_mute(sink.index, false)));
            }
        }
        Kind::Input => {
            let source = input(listing, id)?;
            requests.push(Request::SetSourceVolume(device_volume(source.index, at_level(&source.cvolume, volume))));
            if source.muted {
                requests.push(Request::SetSourceMute(device_mute(source.index, false)));
            }
        }
        Kind::App => {
            for stream in app(listing, id)? {
                // A stream without a volume of its own has none to set.
                if stream.has_volume && stream.volume_writable {
                    let volume = at_level(&stream.cvolume, volume);
                    requests.push(Request::SetSinkInputVolume(SetStreamVolumeParams { index: stream.index, volume }));
                }
                if stream.muted {
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

/// Never an output's monitor: those aren't inputs ([`crate::reading`]).
fn input(listing: &Listing, id: u32) -> Result<&SourceInfo, String> {
    let inputs = listing.sources.iter().filter(|source| source.monitor_of_sink_index.is_none());
    inputs.into_iter().find(|source| source.index == id).ok_or_else(|| format!("there is no input {id}"))
}

/// Every stream of the app `id` names.
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
