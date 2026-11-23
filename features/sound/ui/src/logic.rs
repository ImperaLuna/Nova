use nova_look::{Glyph, OutputKind};
use nova_sound_api::{Device, DeviceKind, Link, Sound};

pub fn current(devices: &[Device]) -> Option<&Device> {
    devices.iter().find(|device| device.current)
}

pub fn output(sound: Option<&Sound>) -> Option<&Device> {
    sound.filter(|sound| sound.available).and_then(|sound| current(&sound.outputs))
}

pub fn input(sound: Option<&Sound>) -> Option<&Device> {
    sound.filter(|sound| sound.available).and_then(|sound| current(&sound.inputs))
}

pub fn output_kind(kind: DeviceKind) -> OutputKind {
    match kind {
        DeviceKind::Headphones => OutputKind::Headphones,
        DeviceKind::Bluetooth => OutputKind::Bluetooth,
        DeviceKind::Display => OutputKind::Display,
        DeviceKind::Speakers => OutputKind::Speakers,
    }
}

pub fn kind_glyph(kind: DeviceKind) -> Glyph {
    match kind {
        DeviceKind::Headphones => Glyph::HEADPHONES,
        DeviceKind::Bluetooth => Glyph::BLUETOOTH,
        DeviceKind::Display => Glyph::MONITOR,
        DeviceKind::Speakers => Glyph::SPEAKER,
    }
}

pub fn link_label(link: Option<Link>) -> &'static str {
    match link {
        Some(Link::Bluetooth) => "Bluetooth",
        Some(Link::Usb) => "USB",
        None => "",
    }
}

fn kind_label(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Headphones => "Headphones",
        DeviceKind::Bluetooth => "Bluetooth",
        DeviceKind::Display => "HDMI or DisplayPort",
        DeviceKind::Speakers => "Speakers",
    }
}

pub fn subtitle(device: &Device, is_input: bool) -> &'static str {
    match (link_label(device.link), is_input) {
        ("", false) => kind_label(device.kind),
        (link, _) => link,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: u32, kind: DeviceKind, link: Option<Link>, current: bool) -> Device {
        Device { id, name: format!("device {id}"), kind, link, volume: 0.5, muted: false, current }
    }

    fn sound(available: bool) -> Sound {
        Sound {
            available,
            outputs: vec![
                device(1, DeviceKind::Speakers, None, false),
                device(2, DeviceKind::Headphones, Some(Link::Usb), true),
            ],
            inputs: vec![device(3, DeviceKind::Speakers, None, true)],
            apps: Vec::new(),
            recorders: Vec::new(),
        }
    }

    #[test]
    fn the_devices_in_use_are_the_current_ones_while_the_server_answers() {
        let up = sound(true);
        assert_eq!(output(Some(&up)).map(|device| device.id), Some(2));
        assert_eq!(input(Some(&up)).map(|device| device.id), Some(3));
        let down = sound(false);
        assert_eq!(output(Some(&down)), None);
        assert_eq!(output(None), None);
    }

    #[test]
    fn a_row_says_its_link_before_its_kind() {
        let headset = device(1, DeviceKind::Headphones, Some(Link::Bluetooth), false);
        assert_eq!(subtitle(&headset, false), "Bluetooth");
        let monitor = device(2, DeviceKind::Display, None, false);
        assert_eq!(subtitle(&monitor, false), "HDMI or DisplayPort");
        assert_eq!(subtitle(&monitor, true), "", "an input says only its link");
        let dongle = device(3, DeviceKind::Speakers, Some(Link::Usb), false);
        assert_eq!(subtitle(&dongle, true), "USB");
    }
}
