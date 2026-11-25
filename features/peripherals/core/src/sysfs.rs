use super::UsbId;
use nova_peripherals_api::PeripheralLink;
use std::path::Path;

const BLUEZ_PREFIX: &str = "/org/bluez/";
const HID_BUS_USB: u16 = 0x0003;
const HID_BUS_BLUETOOTH: u16 = 0x0005;
const MAX_DEPTH: usize = 16;

#[derive(Debug, Default, PartialEq)]
pub struct Location {
    pub link: Option<PeripheralLink>,
    pub usb_ids: Vec<UsbId>,
}

pub fn locate(sys_root: &Path, native_path: &str) -> Location {
    if native_path.starts_with(BLUEZ_PREFIX) {
        return Location { link: Some(PeripheralLink::Bluetooth), usb_ids: Vec::new() };
    }
    if native_path.is_empty() || native_path.contains('/') || native_path == "." || native_path == ".." {
        return Location::default();
    }
    let Ok(device) = sys_root.join("class/power_supply").join(native_path).join("device").canonicalize() else {
        return Location::default();
    };
    let own = device.file_name().and_then(|name| hid_id(&name.to_string_lossy()));
    let in_receiver =
        device.parent().and_then(Path::file_name).is_some_and(|name| hid_id(&name.to_string_lossy()).is_some());
    let ancestors: Vec<&Path> = device.ancestors().take(MAX_DEPTH).collect();
    let through_adapter =
        ancestors.iter().any(|folder| folder.file_name().is_some_and(|name| is_adapter(&name.to_string_lossy())));
    let usb_device = ancestors.iter().find_map(|folder| usb_id(folder));
    let link = match own {
        Some((bus, _)) if bus == HID_BUS_BLUETOOTH => Some(PeripheralLink::Bluetooth),
        _ if through_adapter => Some(PeripheralLink::Bluetooth),
        _ if in_receiver => Some(PeripheralLink::Dongle),
        Some((bus, _)) if bus == HID_BUS_USB => Some(PeripheralLink::Wired),
        _ if usb_device.is_some() => Some(PeripheralLink::Wired),
        _ => None,
    };
    let mut usb_ids = Vec::new();
    if let Some((HID_BUS_USB, id)) = own {
        usb_ids.push(id);
    }
    if let Some(id) = usb_device.filter(|id| !in_receiver && !usb_ids.contains(id)) {
        usb_ids.push(id);
    }
    Location { link, usb_ids }
}

fn hid_id(name: &str) -> Option<(u16, UsbId)> {
    let (ids, _instance) = name.split_once('.')?;
    let mut parts = ids.split(':');
    let mut next = || parts.next().filter(|part| part.len() == 4).and_then(|part| u16::from_str_radix(part, 16).ok());
    let (bus, vendor, product) = (next()?, next()?, next()?);
    parts.next().is_none().then_some((bus, UsbId { vendor, product }))
}

fn is_adapter(name: &str) -> bool {
    name.strip_prefix("hci").is_some_and(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
}

fn usb_id(folder: &Path) -> Option<UsbId> {
    let read = |file: &str| {
        let text = std::fs::read_to_string(folder.join(file)).ok()?;
        u16::from_str_radix(text.trim(), 16).ok()
    };
    Some(UsbId { vendor: read("idVendor")?, product: read("idProduct")? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hid_ids_parse_from_folder_names() {
        assert_eq!(hid_id("0003:046D:407F.0004"), Some((3, UsbId { vendor: 0x046d, product: 0x407f })));
        assert_eq!(hid_id("0005:05AC:0265.000A").map(|(bus, _)| bus), Some(5));
        for name in ["power_supply", "3-2:1.2", "0003:046D.0004", "0003:046D:407F:1.0004", "usb3"] {
            assert_eq!(hid_id(name), None, "{name}");
        }
    }

    #[test]
    fn names_from_outside_stay_in_the_power_supply_folder() {
        let sys = nova_test_support::TempDir::new("peripherals-sysfs-names");
        for name in ["", ".", "..", "../../etc", "a/b"] {
            assert_eq!(locate(&sys, name), Location::default(), "{name}");
        }
        assert_eq!(locate(&sys, "/org/bluez/hci0/dev_AA_BB").link, Some(PeripheralLink::Bluetooth));
    }
}
