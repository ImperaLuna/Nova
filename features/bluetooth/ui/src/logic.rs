use nova_bluetooth_api::{BluetoothDevice, BluetoothKind, BluetoothStatus, PairingAsk};
use nova_look::Glyph;

pub fn icon(kind: BluetoothKind) -> Glyph {
    match kind {
        BluetoothKind::Headphones | BluetoothKind::Headset => Glyph::HEADPHONES,
        BluetoothKind::Speaker => Glyph::SPEAKER,
        BluetoothKind::Keyboard => Glyph::KEYBOARD,
        BluetoothKind::Mouse => Glyph::MOUSE,
        BluetoothKind::Gamepad => Glyph::GAMEPAD,
        BluetoothKind::Tablet | BluetoothKind::Phone => Glyph::DEVICE,
        BluetoothKind::Computer => Glyph::MONITOR,
        BluetoothKind::Other => Glyph::BLUETOOTH,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Connected,
    Mine,
    Nearby,
}

pub fn section(device: &BluetoothDevice) -> Section {
    match device.status {
        BluetoothStatus::Connected | BluetoothStatus::Connecting | BluetoothStatus::Disconnecting => Section::Connected,
        BluetoothStatus::Paired => Section::Mine,
        BluetoothStatus::Nearby | BluetoothStatus::Pairing => Section::Nearby,
    }
}

pub fn in_section(devices: &[BluetoothDevice], key: Section) -> Vec<&BluetoothDevice> {
    devices.iter().filter(|device| section(device) == key).collect()
}

pub fn busy_text(status: BluetoothStatus, waiting: bool) -> Option<&'static str> {
    match status {
        BluetoothStatus::Pairing if waiting => Some("Waiting for you"),
        BluetoothStatus::Pairing => Some("Pairing"),
        BluetoothStatus::Connecting => Some("Connecting"),
        BluetoothStatus::Disconnecting => Some("Disconnecting"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    Connect,
    Disconnect,
}

pub fn click(status: BluetoothStatus) -> Option<Click> {
    match status {
        BluetoothStatus::Nearby | BluetoothStatus::Paired => Some(Click::Connect),
        BluetoothStatus::Connected => Some(Click::Disconnect),
        _ => None,
    }
}

pub fn hover_text(status: BluetoothStatus) -> Option<&'static str> {
    match status {
        BluetoothStatus::Nearby => Some("Pair"),
        BluetoothStatus::Paired => Some("Connect"),
        BluetoothStatus::Connected => Some("Disconnect"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Actions,
    Forget,
}

pub fn menu(status: BluetoothStatus) -> Option<Menu> {
    match status {
        BluetoothStatus::Connected => Some(Menu::Actions),
        BluetoothStatus::Paired => Some(Menu::Forget),
        _ => None,
    }
}

pub fn question(ask: &PairingAsk, name: &str, incoming: bool) -> String {
    match ask {
        PairingAsk::Confirm { .. } if incoming => format!("{name} wants to pair. Does it show this code?"),
        PairingAsk::Confirm { .. } => format!("Does {name} show this code?"),
        PairingAsk::EnterPin => format!("Enter the PIN for {name}"),
        PairingAsk::EnterPasskey => format!("Enter the code {name} shows"),
        PairingAsk::ShowPin { .. } | PairingAsk::ShowPasskey { .. } => {
            format!("Type this on {name}, then press Enter")
        }
        PairingAsk::Authorize => format!("{name} wants to connect. Allow it?"),
    }
}

pub fn code(ask: &PairingAsk) -> Option<&str> {
    match ask {
        PairingAsk::Confirm { code } | PairingAsk::ShowPasskey { code, .. } => Some(code),
        PairingAsk::ShowPin { pin } => Some(pin),
        _ => None,
    }
}

pub fn typed_on_device(ask: &PairingAsk) -> usize {
    match ask {
        PairingAsk::ShowPasskey { typed, .. } => usize::from(*typed),
        _ => 0,
    }
}

pub fn typed(ask: &PairingAsk) -> bool {
    matches!(ask, PairingAsk::EnterPin | PairingAsk::EnterPasskey)
}

const PASSKEY_MAX_DIGITS: usize = 6;

pub fn buttons(ask: &PairingAsk, incoming: bool) -> (&'static str, Option<&'static str>) {
    match ask {
        PairingAsk::ShowPin { .. } | PairingAsk::ShowPasskey { .. } => ("Cancel", None),
        PairingAsk::Authorize => ("Decline", Some("Allow")),
        _ if incoming => ("Decline", Some("Pair")),
        PairingAsk::Confirm { .. } => ("Cancel", Some("It matches")),
        _ => ("Cancel", Some("Pair")),
    }
}

/// How many characters the field for `ask` takes, as today's shell.
pub fn code_max(ask: &PairingAsk) -> usize {
    match ask {
        PairingAsk::EnterPasskey => PASSKEY_MAX_DIGITS,
        _ => nova_bluetooth_api::BLUETOOTH_PIN_MAX_BYTES,
    }
}

pub fn valid_code(ask: &PairingAsk, text: &str) -> bool {
    match ask {
        PairingAsk::EnterPin => {
            (1..=nova_bluetooth_api::BLUETOOTH_PIN_MAX_BYTES).contains(&text.len())
                && text.bytes().all(|byte| byte.is_ascii_graphic())
        }
        PairingAsk::EnterPasskey => {
            let text = text.trim();
            (1..=PASSKEY_MAX_DIGITS).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_digit())
        }
        _ => false,
    }
}

pub fn device_name<'a>(devices: &'a [BluetoothDevice], address: &'a str) -> &'a str {
    devices.iter().find(|device| device.address == address).map_or(address, |device| &device.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(status: BluetoothStatus) -> BluetoothDevice {
        BluetoothDevice {
            address: "00:1B:66:AA:BB:CC".into(),
            name: "WH-1000XM5".into(),
            kind: BluetoothKind::Headphones,
            status,
            battery: None,
            error: None,
        }
    }

    #[test]
    fn devices_go_in_their_sections() {
        use BluetoothStatus::*;
        let sections: Vec<_> = [Connected, Connecting, Disconnecting, Paired, Nearby, Pairing]
            .map(|status| section(&device(status)))
            .into();
        use Section::{Connected as C, Mine as M, Nearby as N};
        assert_eq!(sections, [C, C, C, M, N, N]);
        let devices = [device(Paired), device(Connected), device(Paired)];
        assert_eq!(in_section(&devices, Section::Mine).len(), 2);
    }

    #[test]
    fn a_row_says_what_novad_is_doing_and_what_a_click_does() {
        assert_eq!(busy_text(BluetoothStatus::Pairing, true), Some("Waiting for you"));
        assert_eq!(busy_text(BluetoothStatus::Pairing, false), Some("Pairing"));
        assert_eq!(busy_text(BluetoothStatus::Connected, false), None);
        assert_eq!(click(BluetoothStatus::Nearby), Some(Click::Connect));
        assert_eq!(click(BluetoothStatus::Connected), Some(Click::Disconnect));
        assert_eq!(click(BluetoothStatus::Connecting), None);
        assert_eq!(menu(BluetoothStatus::Connected), Some(Menu::Actions));
        assert_eq!(menu(BluetoothStatus::Paired), Some(Menu::Forget));
        assert_eq!(menu(BluetoothStatus::Nearby), None);
        assert_eq!(hover_text(BluetoothStatus::Nearby), Some("Pair"));
        assert_eq!(icon(BluetoothKind::Computer), Glyph::MONITOR);
        assert_eq!(icon(BluetoothKind::Other), Glyph::BLUETOOTH);
    }

    #[test]
    fn a_pairing_question_reads_as_bluetooth_js_asks_it() {
        let confirm = PairingAsk::Confirm { code: "482913".into() };
        assert_eq!(question(&confirm, "Pixel", false), "Does Pixel show this code?");
        assert_eq!(question(&confirm, "Pixel", true), "Pixel wants to pair. Does it show this code?");
        assert_eq!(code(&confirm), Some("482913"));
        assert_eq!(buttons(&confirm, false), ("Cancel", Some("It matches")));
        assert_eq!(buttons(&confirm, true), ("Decline", Some("Pair")));
        let show = PairingAsk::ShowPasskey { code: "123456".into(), typed: 2 };
        assert_eq!(buttons(&show, false), ("Cancel", None));
        assert_eq!(typed_on_device(&show), 2);
        assert_eq!(buttons(&PairingAsk::Authorize, true), ("Decline", Some("Allow")));
        assert_eq!(buttons(&PairingAsk::EnterPin, false), ("Cancel", Some("Pair")));
        assert!(typed(&PairingAsk::EnterPasskey) && !typed(&confirm));
        assert_eq!(code(&PairingAsk::ShowPin { pin: "0000".into() }), Some("0000"));
    }

    #[test]
    fn a_typed_code_must_fit_bluez_s_limits() {
        assert!(valid_code(&PairingAsk::EnterPin, "0000"));
        assert!(!valid_code(&PairingAsk::EnterPin, ""));
        assert!(!valid_code(&PairingAsk::EnterPin, "a b"));
        assert!(!valid_code(&PairingAsk::EnterPin, &"1".repeat(17)));
        assert_eq!(code_max(&PairingAsk::EnterPin), 16);
        assert_eq!(code_max(&PairingAsk::EnterPasskey), 6);
        assert!(valid_code(&PairingAsk::EnterPasskey, " 123456 "));
        assert!(!valid_code(&PairingAsk::EnterPasskey, "1234567"));
        assert!(!valid_code(&PairingAsk::EnterPasskey, "12a"));
        assert!(!valid_code(&PairingAsk::Authorize, "1"));
    }

    #[test]
    fn a_prompt_names_a_device_it_doesn_t_list_by_its_address() {
        let devices = [device(BluetoothStatus::Pairing)];
        assert_eq!(device_name(&devices, "00:1B:66:AA:BB:CC"), "WH-1000XM5");
        assert_eq!(device_name(&devices, "11:22"), "11:22");
    }
}
