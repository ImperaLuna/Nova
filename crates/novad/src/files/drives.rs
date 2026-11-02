//! Which of udisks2's filesystems Nova Files' sidebar lists, and what it
//! calls them: the rules alone, over what `udisks.rs` read from the bus,
//! so they're checked without one.
//!
//! Listed is what a person would open, as GNOME's file manager decides it:
//! a filesystem udisks2 doesn't hide (`HintIgnore`), that isn't a boot
//! partition, and that is either mounted somewhere people browse (`/`, or
//! under `/media`, `/run/media`, `/mnt` or the home folder) or not mounted
//! and not part of the system (`HintSystem` false: a USB stick, a card).
//! The root filesystem is "System"; the rest go by their label, then their
//! drive's model, then their size. Disks inside the machine come first,
//! then the removable ones, each by name.

use crate::files::folder::natural;
use nova_protocol::Drive;
use std::collections::BTreeMap;
use std::path::Path;

/// What the root filesystem is called.
const SYSTEM_NAME: &str = "System";
/// Where mounts people browse live, besides `/` and the home folder.
const BROWSED_MOUNTS: &[&str] = &["/media", "/run/media", "/mnt"];
/// Partition types of boot partitions, which are never listed: the EFI
/// system partition (GPT, then MBR) and BIOS boot (GPT).
const BOOT_PARTITION_TYPES: &[&str] =
    &["c12a7328-f81f-11d2-ba4b-00a0c93ec93b", "0xef", "21686148-6449-6e6f-744e-656564454649"];
/// The bus a USB drive is on (`ConnectionBus`).
const USB_BUS: &str = "usb";

/// A block device udisks2 knows, as far as the sidebar cares.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Block {
    /// Its drive's object, `/` when it has none (a loop device).
    pub drive: String,
    pub size: u64,
    pub label: String,
    /// The name udisks2 suggests showing (`HintName`), maybe empty.
    pub hint_name: String,
    pub hint_ignore: bool,
    pub hint_system: bool,
    /// Where it's mounted, when it holds a filesystem udisks2 can mount;
    /// none when it holds none.
    pub mounts: Option<Vec<String>>,
    /// Its partition's type, when it's a partition.
    pub partition_type: Option<String>,
}

/// A drive (a disk, a stick, a card reader) udisks2 knows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DriveInfo {
    pub vendor: String,
    pub model: String,
    /// Its media can be taken out, as udisks2 says (`Removable`).
    pub removable: bool,
    /// `usb`, `sdio`, `ieee1394` or empty.
    pub connection_bus: String,
    pub can_power_off: bool,
    pub ejectable: bool,
}

impl DriveInfo {
    /// It can be unplugged, so it can be ejected.
    pub fn unpluggable(&self) -> bool {
        self.removable || self.connection_bus == USB_BUS
    }
}

/// What udisks2 knows, by object name (the last part of its path, like
/// `sdb1`): its block devices and its drives.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Known {
    pub blocks: BTreeMap<String, Block>,
    /// By object path.
    pub drives: BTreeMap<String, DriveInfo>,
}

impl Known {
    /// The drive of block `id`, if it has one.
    pub fn drive_of(&self, id: &str) -> Option<(&str, &DriveInfo)> {
        let block = self.blocks.get(id)?;
        self.drives.get_key_value(&block.drive).map(|(path, drive)| (path.as_str(), drive))
    }

    /// The listed filesystems on drive `drive` (an object path) and where
    /// each is mounted: what ejecting it unmounts.
    pub fn mounted_on(&self, drive: &str) -> Vec<(&str, &[String])> {
        self.blocks
            .iter()
            .filter(|(_, block)| block.drive == drive)
            .filter_map(|(id, block)| Some((id.as_str(), block.mounts.as_deref().filter(|mounts| !mounts.is_empty())?)))
            .collect()
    }
}

/// The drives the sidebar lists, in its order. `used` says how full the
/// filesystem mounted at a folder is, as (size, used) bytes.
pub fn listed(known: &Known, home: Option<&Path>, used: impl Fn(&str) -> Option<(u64, u64)>) -> Vec<Drive> {
    let mut drives: Vec<Drive> = known
        .blocks
        .iter()
        .filter_map(|(id, block)| {
            let mounts = block.mounts.as_ref()?;
            if block.hint_ignore || block.partition_type.as_deref().is_some_and(is_boot_partition) {
                return None;
            }
            let mount = match mounts.first() {
                // The root's own mount first, so it's "System" whatever
                // else it's also mounted at.
                Some(_) => Some(
                    mounts
                        .iter()
                        .find(|mount| mount.as_str() == "/")
                        .or_else(|| mounts.iter().find(|mount| browsed(mount, home)))?
                        .clone(),
                ),
                None if block.hint_system => return None,
                None => None,
            };
            let drive = known.drives.get(&block.drive);
            let usage = mount.as_deref().and_then(&used);
            Some(Drive {
                id: id.clone(),
                name: name(block, drive, mount.as_deref()),
                removable: drive.is_some_and(DriveInfo::unpluggable),
                size: usage.map_or(block.size, |(size, _)| size),
                used: usage.map(|(_, used)| used),
                mount,
            })
        })
        .collect();
    drives.sort_by(|a, b| a.removable.cmp(&b.removable).then_with(|| natural(&a.name, &b.name)));
    drives
}

fn is_boot_partition(kind: &str) -> bool {
    BOOT_PARTITION_TYPES.iter().any(|boot| boot.eq_ignore_ascii_case(kind))
}

/// Whether `mount` (wire text) is somewhere people browse.
fn browsed(mount: &str, home: Option<&Path>) -> bool {
    let mount = &nova_protocol::wire_path(mount);
    mount == Path::new("/")
        || BROWSED_MOUNTS.iter().any(|place| mount.starts_with(place))
        || home.is_some_and(|home| mount.starts_with(home))
}

/// What the sidebar calls `block`.
fn name(block: &Block, drive: Option<&DriveInfo>, mount: Option<&str>) -> String {
    if mount == Some("/") {
        return SYSTEM_NAME.into();
    }
    let model = drive.map(|drive| [drive.vendor.trim(), drive.model.trim()].join(" ").trim().to_string());
    [block.hint_name.trim(), block.label.trim(), model.as_deref().unwrap_or_default()]
        .into_iter()
        .find(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} Volume", decimal_size(block.size)))
}

/// `bytes` as drives are sold: decimal units, a whole number.
fn decimal_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1000.0 && unit < UNITS.len() - 1 {
        size /= 1000.0;
        unit += 1;
    }
    format!("{:.0} {}", size, UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/user";
    const STICK: &str = "/org/freedesktop/UDisks2/drives/SanDisk_Cruzer_Blade_4C530001";
    const SSD: &str = "/org/freedesktop/UDisks2/drives/Samsung_SSD_990_PRO_1TB";

    fn block(drive: &str, mounts: &[&str]) -> Block {
        Block {
            drive: drive.into(),
            size: 32_000_000_000,
            mounts: Some(mounts.iter().map(|mount| mount.to_string()).collect()),
            ..Block::default()
        }
    }

    fn known(blocks: &[(&str, Block)]) -> Known {
        let stick = DriveInfo {
            vendor: "SanDisk".into(),
            model: "Cruzer Blade".into(),
            removable: true,
            connection_bus: USB_BUS.into(),
            can_power_off: true,
            ejectable: true,
        };
        let ssd = DriveInfo { vendor: String::new(), model: "Samsung SSD 990 PRO 1TB".into(), ..DriveInfo::default() };
        Known {
            blocks: blocks.iter().map(|(id, block)| (id.to_string(), block.clone())).collect(),
            drives: [(STICK.to_string(), stick), (SSD.to_string(), ssd)].into(),
        }
    }

    fn names(known: &Known) -> Vec<String> {
        listed(known, Some(Path::new(HOME)), |_| None).into_iter().map(|drive| drive.name).collect()
    }

    #[test]
    fn the_root_is_system_and_disks_come_before_sticks() {
        let known = known(&[
            ("sdb1", Block { label: "SANDISK".into(), ..block(STICK, &[]) }),
            ("nvme0n1p2", Block { hint_system: true, ..block(SSD, &["/"]) }),
            ("nvme1n1p1", Block { label: "Games SSD".into(), hint_system: true, ..block(SSD, &["/mnt/games"]) }),
        ]);
        assert_eq!(names(&known), ["Games SSD", "System", "SANDISK"]);
    }

    #[test]
    fn boot_hidden_and_unbrowsed_filesystems_are_left_out() {
        let efi = Block { partition_type: Some("C12A7328-F81F-11D2-BA4B-00A0C93EC93B".into()), ..block(SSD, &[]) };
        let known = known(&[
            ("nvme0n1p1", efi),
            ("nvme0n1p3", Block { hint_ignore: true, ..block(SSD, &[]) }),
            ("nvme0n1p4", Block { hint_system: true, ..block(SSD, &["/boot"]) }),
            ("nvme0n1p5", Block { hint_system: true, ..block(SSD, &[]) }),
            ("nvme0n1p6", Block { mounts: None, ..block(SSD, &[]) }),
            ("sdb1", block(STICK, &["/run/media/user/SANDISK"])),
        ]);
        assert_eq!(names(&known), ["SanDisk Cruzer Blade"]);
    }

    #[test]
    fn the_root_is_system_wherever_else_it_is_mounted() {
        let known = known(&[("nvme0n1p2", Block { hint_system: true, ..block(SSD, &["/mnt/snapshots", "/"]) })]);
        let drives = listed(&known, None, |_| None);
        assert_eq!((drives[0].name.as_str(), drives[0].mount.as_deref()), ("System", Some("/")));
    }

    #[test]
    fn a_filesystem_without_a_label_or_model_goes_by_its_size() {
        let known = known(&[("loop0", block("/", &[]))]);
        assert_eq!(names(&known), ["32 GB Volume"]);
    }

    #[test]
    fn a_mounted_drive_says_how_full_it_is() {
        let known = known(&[("sdb1", block(STICK, &["/run/media/user/SANDISK"]))]);
        let drives = listed(&known, None, |mount| (mount == "/run/media/user/SANDISK").then_some((31_000, 7_000)));
        let drive = &drives[0];
        assert_eq!(
            (drive.mount.as_deref(), drive.size, drive.used),
            (Some("/run/media/user/SANDISK"), 31_000, Some(7_000))
        );
        assert!(drive.removable);
    }

    #[test]
    fn ejecting_unmounts_every_mounted_filesystem_of_the_drive() {
        let known = known(&[
            ("sdb1", block(STICK, &["/run/media/user/A"])),
            ("sdb2", block(STICK, &[])),
            ("sdb3", block(STICK, &["/run/media/user/C"])),
            ("nvme0n1p2", block(SSD, &["/"])),
        ]);
        let mounted: Vec<&str> = known.mounted_on(STICK).into_iter().map(|(id, _)| id).collect();
        assert_eq!(mounted, ["sdb1", "sdb3"]);
    }
}
