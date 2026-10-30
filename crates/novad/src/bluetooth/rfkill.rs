//! The kernel's radio switches (rfkill) for Bluetooth: whether a software
//! block (`rfkill block`, a desktop's airplane mode) or a hardware switch
//! keeps the adapter off, and lifting the software block.
//!
//! The state is read from `/sys/class/rfkill/rfkill*/{type,soft,hard}`.
//! It's read again whenever BlueZ says something changed: blocking or
//! unblocking powers the adapter off or back, which BlueZ reports. The block
//! is lifted by writing a change-all event for Bluetooth to `/dev/rfkill`,
//! which systemd's `uaccess` rule lets the seat's user open.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

/// `RFKILL_TYPE_BLUETOOTH` in `linux/rfkill.h`.
const TYPE_BLUETOOTH: u8 = 2;
/// `RFKILL_OP_CHANGE_ALL`: sets every switch of the type.
const OP_CHANGE_ALL: u8 = 3;
/// What `type` holds for a Bluetooth switch.
const TYPE_NAME: &str = "bluetooth";

/// Where the switches are: the machine's, or stand-ins in tests.
#[derive(Clone)]
pub struct Rfkill {
    /// `/sys/class/rfkill`.
    pub class: PathBuf,
    /// `/dev/rfkill`.
    pub device: PathBuf,
}

/// What keeps Bluetooth off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Blocked {
    pub soft: bool,
    pub hard: bool,
}

impl Rfkill {
    pub fn system() -> Self {
        Self { class: "/sys/class/rfkill".into(), device: "/dev/rfkill".into() }
    }

    /// Whether any Bluetooth switch is blocked. No switches (no adapter, or
    /// no rfkill in the kernel) means nothing blocks it.
    pub fn blocked(&self) -> Blocked {
        let mut blocked = Blocked::default();
        let Ok(entries) = std::fs::read_dir(&self.class) else { return blocked };
        for entry in entries.flatten() {
            let read = |name: &str| std::fs::read_to_string(entry.path().join(name)).unwrap_or_default();
            if read("type").trim() != TYPE_NAME {
                continue;
            }
            blocked.soft |= read("soft").trim() == "1";
            blocked.hard |= read("hard").trim() == "1";
        }
        blocked
    }

    /// Lifts the software block on every Bluetooth switch. A hardware switch
    /// stays as it is.
    pub fn unblock(&self) -> Result<(), String> {
        // struct rfkill_event: idx (u32), type, op, soft, hard (u8 each).
        let event = [0, 0, 0, 0, TYPE_BLUETOOTH, OP_CHANGE_ALL, 0, 0];
        OpenOptions::new()
            .write(true)
            .open(&self.device)
            .and_then(|mut device| device.write_all(&event))
            .map_err(|e| format!("can't unblock Bluetooth through {}: {e}", self.device.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    fn switch(class: &std::path::Path, name: &str, kind: &str, soft: bool, hard: bool) {
        let dir = class.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("type"), format!("{kind}\n")).unwrap();
        std::fs::write(dir.join("soft"), format!("{}\n", u8::from(soft))).unwrap();
        std::fs::write(dir.join("hard"), format!("{}\n", u8::from(hard))).unwrap();
    }

    #[test]
    fn only_bluetooth_switches_count() {
        let dir = TempDir::new("rfkill-blocked");
        let rfkill = Rfkill { class: dir.join("class"), device: dir.join("rfkill") };
        assert_eq!(rfkill.blocked(), Blocked::default(), "no switches block nothing");
        switch(&rfkill.class, "rfkill0", "wlan", true, true);
        switch(&rfkill.class, "rfkill1", "bluetooth", true, false);
        assert_eq!(rfkill.blocked(), Blocked { soft: true, hard: false });
        switch(&rfkill.class, "rfkill2", "bluetooth", false, true);
        assert_eq!(rfkill.blocked(), Blocked { soft: true, hard: true });
    }

    #[test]
    fn unblocking_writes_a_change_all_event_for_bluetooth() {
        let dir = TempDir::new("rfkill-unblock");
        let rfkill = Rfkill { class: dir.join("class"), device: dir.join("rfkill") };
        assert!(rfkill.unblock().is_err(), "no device to write to");
        std::fs::write(&rfkill.device, b"").unwrap();
        rfkill.unblock().unwrap();
        assert_eq!(std::fs::read(&rfkill.device).unwrap(), [0, 0, 0, 0, 2, 3, 0, 0]);
    }
}
