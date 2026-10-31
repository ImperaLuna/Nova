//! The kernel's radio switches (rfkill) for Bluetooth: whether a software
//! block (`rfkill block`, a desktop's airplane mode) or a hardware switch
//! keeps the adapter off, and lifting the software block.
//!
//! The state is read from `/sys/class/rfkill/rfkill*/{type,soft,hard}`,
//! only the switches of the adapter in use when the kernel says which they
//! are (`/sys/class/bluetooth/hci0/rfkill*`): another adapter's block
//! doesn't keep this one off. It's read again whenever BlueZ says something
//! changed: blocking or unblocking powers the adapter off or back, which
//! BlueZ reports. The block is lifted by writing rfkill events to
//! `/dev/rfkill`, which systemd's `uaccess` rule lets the seat's user open.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

/// `RFKILL_TYPE_BLUETOOTH` in `linux/rfkill.h`.
const TYPE_BLUETOOTH: u8 = 2;
/// `RFKILL_OP_CHANGE`: sets the switch `idx`.
const OP_CHANGE: u8 = 2;
/// `RFKILL_OP_CHANGE_ALL`: sets every switch of the type.
const OP_CHANGE_ALL: u8 = 3;
/// How a switch's directory is named: this, then its index.
const SWITCH_PREFIX: &str = "rfkill";
/// What `type` holds for a Bluetooth switch.
const TYPE_NAME: &str = "bluetooth";

/// Where the switches are: the machine's, or stand-ins in tests.
#[derive(Clone)]
pub struct Rfkill {
    /// `/sys/class/rfkill`.
    pub class: PathBuf,
    /// `/dev/rfkill`.
    pub device: PathBuf,
    /// `/sys/class/bluetooth`: each adapter's own switches.
    pub adapters: PathBuf,
}

/// What keeps Bluetooth off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Blocked {
    pub soft: bool,
    pub hard: bool,
}

impl Rfkill {
    pub fn system() -> Self {
        Self {
            class: "/sys/class/rfkill".into(),
            device: "/dev/rfkill".into(),
            adapters: "/sys/class/bluetooth".into(),
        }
    }

    /// The indexes of the switches of `adapter` (`hci0`), or None when the
    /// kernel doesn't say which they are.
    fn own(&self, adapter: Option<&str>) -> Option<Vec<u32>> {
        let entries = std::fs::read_dir(self.adapters.join(adapter?)).ok()?;
        let own: Vec<u32> = entries.flatten().filter_map(|entry| index(&entry.file_name().to_string_lossy())).collect();
        (!own.is_empty()).then_some(own)
    }

    /// Whether a switch of `adapter` (`hci0`) is blocked; every Bluetooth
    /// switch counts when the kernel doesn't say which are its own. No
    /// switches (no adapter, or no rfkill in the kernel) means nothing
    /// blocks it.
    pub fn blocked(&self, adapter: Option<&str>) -> Blocked {
        let own = self.own(adapter);
        let mut blocked = Blocked::default();
        let Ok(entries) = std::fs::read_dir(&self.class) else { return blocked };
        for entry in entries.flatten() {
            let read = |name: &str| std::fs::read_to_string(entry.path().join(name)).unwrap_or_default();
            if read("type").trim() != TYPE_NAME {
                continue;
            }
            let idx = index(&entry.file_name().to_string_lossy());
            if own.as_ref().is_some_and(|own| !idx.is_some_and(|idx| own.contains(&idx))) {
                continue;
            }
            blocked.soft |= read("soft").trim() == "1";
            blocked.hard |= read("hard").trim() == "1";
        }
        blocked
    }

    /// Lifts the software block on the switches of `adapter` (`hci0`), or on
    /// every Bluetooth switch when the kernel doesn't say which are its own.
    /// A hardware switch stays as it is.
    pub fn unblock(&self, adapter: Option<&str>) -> Result<(), String> {
        // struct rfkill_event: idx (u32), type, op, soft, hard (u8 each).
        let event = |idx: u32, op: u8| {
            let [a, b, c, d] = idx.to_ne_bytes();
            [a, b, c, d, TYPE_BLUETOOTH, op, 0, 0]
        };
        let events: Vec<u8> = match self.own(adapter) {
            Some(own) => own.into_iter().flat_map(|idx| event(idx, OP_CHANGE)).collect(),
            None => event(0, OP_CHANGE_ALL).to_vec(),
        };
        let mut device = OpenOptions::new()
            .write(true)
            .open(&self.device)
            .map_err(|e| format!("can't unblock Bluetooth through {}: {e}", self.device.display()))?;
        // One event per write: the kernel reads one at a time.
        for event in events.chunks(8) {
            device
                .write_all(event)
                .map_err(|e| format!("can't unblock Bluetooth through {}: {e}", self.device.display()))?;
        }
        Ok(())
    }
}

/// The index in a switch's name (`rfkill3`).
fn index(name: &str) -> Option<u32> {
    name.strip_prefix(SWITCH_PREFIX)?.parse().ok()
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
        let rfkill = stand_in(&dir);
        assert_eq!(rfkill.blocked(None), Blocked::default(), "no switches block nothing");
        switch(&rfkill.class, "rfkill0", "wlan", true, true);
        switch(&rfkill.class, "rfkill1", "bluetooth", true, false);
        assert_eq!(rfkill.blocked(None), Blocked { soft: true, hard: false });
        switch(&rfkill.class, "rfkill2", "bluetooth", false, true);
        assert_eq!(rfkill.blocked(Some("hci0")), Blocked { soft: true, hard: true }, "the kernel doesn't say whose");
    }

    #[test]
    fn another_adapters_block_does_not_count() {
        let dir = TempDir::new("rfkill-own");
        let rfkill = stand_in(&dir);
        switch(&rfkill.class, "rfkill1", "bluetooth", false, true);
        switch(&rfkill.class, "rfkill4", "bluetooth", true, false);
        std::fs::create_dir_all(rfkill.adapters.join("hci1/rfkill4")).unwrap();
        assert_eq!(rfkill.blocked(Some("hci1")), Blocked { soft: true, hard: false });
        std::fs::write(&rfkill.device, b"").unwrap();
        rfkill.unblock(Some("hci1")).unwrap();
        let mut event = 4u32.to_ne_bytes().to_vec();
        event.extend([2, 2, 0, 0]);
        assert_eq!(std::fs::read(&rfkill.device).unwrap(), event, "only its own switch");
    }

    fn stand_in(dir: &std::path::Path) -> Rfkill {
        Rfkill { class: dir.join("class"), device: dir.join("rfkill"), adapters: dir.join("bluetooth") }
    }

    #[test]
    fn unblocking_writes_a_change_all_event_for_bluetooth() {
        let dir = TempDir::new("rfkill-unblock");
        let rfkill = stand_in(&dir);
        assert!(rfkill.unblock(None).is_err(), "no device to write to");
        std::fs::write(&rfkill.device, b"").unwrap();
        rfkill.unblock(None).unwrap();
        assert_eq!(std::fs::read(&rfkill.device).unwrap(), [0, 0, 0, 0, 2, 3, 0, 0]);
    }
}
