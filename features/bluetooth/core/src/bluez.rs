use super::MAX_NAME_CHARS;
use nova_bluetooth_api::{BluetoothDevice, BluetoothKind, BluetoothStatus};
use std::collections::{BTreeMap, HashMap};
use zbus::wire::{OwnedObjectPath, OwnedVariant};

pub const BLUEZ: &str = "org.bluez";
pub const ADAPTER: &str = "org.bluez.Adapter1";
pub const DEVICE: &str = "org.bluez.Device1";
pub const BATTERY: &str = "org.bluez.Battery1";
pub const AGENT_MANAGER: &str = "org.bluez.AgentManager1";
pub const MEDIA_TRANSPORT: &str = "org.bluez.MediaTransport1";
pub const A2DP_SINK: &str = "0000110b-0000-1000-8000-00805f9b34fb";
pub const A2DP_SOURCE: &str = "0000110a-0000-1000-8000-00805f9b34fb";
pub const BLUEZ_PATH: &str = "/org/bluez";

const CLASSIC_AUDIO: [&str; 3] =
    [A2DP_SINK, "0000111e-0000-1000-8000-00805f9b34fb", "00001108-0000-1000-8000-00805f9b34fb"];

pub type Properties = HashMap<String, OwnedVariant>;
pub type Interfaces = HashMap<String, Properties>;

#[derive(Default)]
pub struct Objects(BTreeMap<String, Interfaces>);

pub struct Adapter {
    pub path: OwnedObjectPath,
    pub powered: bool,
    pub discovering: bool,
}

pub struct Device {
    pub path: OwnedObjectPath,
    pub address: String,
    pub name: Option<String>,
    pub paired: bool,
    pub connected: bool,
    seen: bool,
    pub uuids: Vec<String>,
    icon: String,
    battery: Option<u8>,
}

impl Objects {
    pub fn replace(&mut self, objects: HashMap<OwnedObjectPath, Interfaces>) {
        self.0 = objects.into_iter().map(|(path, interfaces)| (path.to_string(), interfaces)).collect();
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn added(&mut self, path: OwnedObjectPath, interfaces: Interfaces) {
        self.0.entry(path.to_string()).or_default().extend(interfaces);
    }

    pub fn removed(&mut self, path: &OwnedObjectPath, interfaces: &[String]) {
        let Some(object) = self.0.get_mut(path.as_str()) else { return };
        for interface in interfaces {
            object.remove(interface);
        }
        if object.is_empty() {
            self.0.remove(path.as_str());
        }
    }

    pub fn changed(&mut self, path: &OwnedObjectPath, interface: &str, changed: Properties, invalidated: &[String]) {
        let Some(properties) = self.0.get_mut(path.as_str()).and_then(|object| object.get_mut(interface)) else {
            return;
        };
        properties.extend(changed);
        for name in invalidated {
            properties.remove(name);
        }
    }

    pub fn adapter(&self) -> Option<Adapter> {
        self.0.iter().find_map(|(path, object)| {
            let adapter = object.get(ADAPTER)?;
            Some(Adapter {
                path: owned(path)?,
                powered: flag(adapter, "Powered"),
                discovering: flag(adapter, "Discovering"),
            })
        })
    }

    pub fn devices(&self) -> Vec<Device> {
        let Some(adapter) = self.adapter() else { return Vec::new() };
        let adapter = adapter.path;
        self.0
            .iter()
            .filter_map(|(path, object)| {
                let device = object.get(DEVICE)?;
                let on_adapter = text(device, "Adapter").is_some_and(|owner| owner == adapter.as_str());
                if !on_adapter {
                    return None;
                }
                Some(Device {
                    path: owned(path)?,
                    address: text(device, "Address")?,
                    name: text(device, "Name")
                        .map(|name| text(device, "Alias").unwrap_or(name))
                        .and_then(|name| clean(&name)),
                    paired: flag(device, "Paired"),
                    connected: flag(device, "Connected"),
                    seen: device.contains_key("RSSI"),
                    uuids: device
                        .get("UUIDs")
                        .and_then(|value| <Vec<String>>::try_from(value.try_clone().ok()?).ok())
                        .unwrap_or_default(),
                    icon: text(device, "Icon").unwrap_or_default(),
                    battery: object
                        .get(BATTERY)
                        .and_then(|battery| battery.get("Percentage"))
                        .and_then(|value| u8::try_from(value).ok())
                        .map(|percent| percent.min(100)),
                })
            })
            .collect()
    }

    pub fn device(&self, address: &str) -> Option<Device> {
        self.devices().into_iter().find(|device| device.address.eq_ignore_ascii_case(address))
    }

    pub fn a2dp(&self, path: &str) -> bool {
        self.0.iter().any(|(object, interfaces)| {
            object.strip_prefix(path).is_some_and(|rest| rest.starts_with('/'))
                && interfaces
                    .get(MEDIA_TRANSPORT)
                    .and_then(|transport| text(transport, "UUID"))
                    .is_some_and(|uuid| [A2DP_SOURCE, A2DP_SINK].iter().any(|a2dp| uuid.eq_ignore_ascii_case(a2dp)))
        })
    }

    pub fn address(&self, path: &str) -> Option<String> {
        text(self.0.get(path)?.get(DEVICE)?, "Address")
    }
}

impl Device {
    pub fn listed(&self) -> bool {
        self.paired || self.connected || (self.seen && self.name.is_some())
    }

    pub fn classic_profile(&self) -> Option<&str> {
        CLASSIC_AUDIO
            .iter()
            .find_map(|wanted| self.uuids.iter().find(|uuid| uuid.eq_ignore_ascii_case(wanted)))
            .map(String::as_str)
    }

    pub fn offers_a2dp(&self) -> bool {
        self.uuids.iter().any(|uuid| uuid.eq_ignore_ascii_case(A2DP_SINK))
    }

    pub fn event(&self, busy: Option<BluetoothStatus>, error: Option<String>) -> BluetoothDevice {
        let status = busy.unwrap_or(match (self.connected, self.paired) {
            (true, _) => BluetoothStatus::Connected,
            (false, true) => BluetoothStatus::Paired,
            (false, false) => BluetoothStatus::Nearby,
        });
        BluetoothDevice {
            address: self.address.clone(),
            name: self.name.clone().unwrap_or_else(|| self.address.clone()),
            kind: kind(&self.icon),
            status,
            battery: self.battery.filter(|_| self.connected),
            error,
        }
    }
}

pub fn order(device: &BluetoothDevice) -> (u8, String) {
    let group = match device.status {
        BluetoothStatus::Connected | BluetoothStatus::Disconnecting => 0,
        BluetoothStatus::Paired | BluetoothStatus::Connecting => 1,
        BluetoothStatus::Nearby | BluetoothStatus::Pairing => 2,
    };
    (group, device.name.to_lowercase())
}

fn kind(icon: &str) -> BluetoothKind {
    match icon {
        "audio-headphones" => BluetoothKind::Headphones,
        "audio-headset" => BluetoothKind::Headset,
        "audio-card" | "audio-speakers" | "multimedia-player" => BluetoothKind::Speaker,
        "input-keyboard" => BluetoothKind::Keyboard,
        "input-mouse" => BluetoothKind::Mouse,
        "input-gaming" => BluetoothKind::Gamepad,
        "input-tablet" => BluetoothKind::Tablet,
        "phone" => BluetoothKind::Phone,
        "computer" => BluetoothKind::Computer,
        _ => BluetoothKind::Other,
    }
}

fn owned(path: &str) -> Option<OwnedObjectPath> {
    OwnedObjectPath::try_from(path).ok()
}

fn flag(properties: &Properties, name: &str) -> bool {
    properties.get(name).and_then(|value| bool::try_from(value).ok()).unwrap_or(false)
}

fn text(properties: &Properties, name: &str) -> Option<String> {
    let value = properties.get(name)?;
    <&str>::try_from(value)
        .map(str::to_owned)
        .ok()
        .or_else(|| OwnedObjectPath::try_from(value.try_clone().ok()?).ok().map(|path| path.to_string()))
}

fn clean(name: &str) -> Option<String> {
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME_CHARS).collect();
    (!name.is_empty()).then_some(name)
}

pub fn is_address(text: &str) -> bool {
    let parts: Vec<&str> = text.split(':').collect();
    parts.len() == 6 && parts.iter().all(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit()))
}

pub fn failure(error: &zbus::Error) -> Option<String> {
    let name = match error {
        zbus::Error::MethodError(name, ..) => name.as_str().to_owned(),
        zbus::Error::FDO(error) if matches!(**error, zbus::fdo::Error::NoReply(_)) => {
            return Some("No answer".into());
        }
        _ => return Some(format!("Something went wrong: {error}")),
    };
    let detail = match error {
        zbus::Error::MethodError(_, detail, _) => detail.clone().unwrap_or_default(),
        _ => String::new(),
    };
    Some(match name.trim_start_matches("org.bluez.Error.") {
        "AuthenticationCanceled" | "AuthenticationRejected" | "Canceled" => return None,
        "AuthenticationFailed" | "AuthenticationTimeout" | "ConnectionAttemptFailed" => "Couldn't pair".into(),
        "NotReady" => "Still starting".into(),
        "Blocked" => "Blocked".into(),
        "AlreadyConnected" | "AlreadyExists" | "InProgress" => "Already under way".into(),
        "DoesNotExist" => "Out of range".into(),
        _ if detail.contains("connection-canceled") => return None,
        _ if detail.contains("le-connection-abort-by-local") => "Couldn't connect".into(),
        _ if detail.contains("page-timeout") || detail.contains("Page Timeout") || detail.contains("Host is down") => {
            "Out of reach".into()
        }
        _ if detail.contains("profile-unavailable") || detail.contains("Protocol not available") => {
            "Not supported".into()
        }
        _ => format!("Something went wrong: {}", if detail.is_empty() { name } else { detail }),
    })
}

pub fn le_aborted(error: &zbus::Error) -> bool {
    matches!(error, zbus::Error::MethodError(_, Some(detail), _) if detail.contains("le-connection-abort-by-local"))
}

pub fn adapter_name(path: &OwnedObjectPath) -> &str {
    path.as_str().rsplit('/').next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_six_hex_pairs() {
        assert!(is_address("9C:2a:70:11:22:33"));
        for bad in ["", "9C:2A:70:11:22", "9C:2A:70:11:22:33:44", "9C-2A-70-11-22-33", "9C:2A:70:11:22:3G", "/org"] {
            assert!(!is_address(bad), "{bad:?}");
        }
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean("  WH-1000XM5\n"), Some("WH-1000XM5".into()));
        assert_eq!(clean(" \u{7} "), None);
        assert_eq!(clean(&"x".repeat(500)).map(|name| name.len()), Some(MAX_NAME_CHARS));
    }

    #[test]
    fn icons_say_what_a_device_is() {
        assert_eq!(kind("audio-headphones"), BluetoothKind::Headphones);
        assert_eq!(kind("input-gaming"), BluetoothKind::Gamepad);
        assert_eq!(kind(""), BluetoothKind::Other);
    }
}
