//! The machine's GPUs, which pick a feature's packages (`Variant` `when`)
//! and how novad starts the UI. Read below a root folder (`/` for real), so
//! tests run on a fake `/sys`.

use crate::Gpu;
use std::path::Path;

pub const DRM_DIR: &str = "sys/class/drm";
/// Present while NVIDIA's own (proprietary or open) kernel driver is loaded;
/// nouveau doesn't create it.
pub const NVIDIA_MODULE: &str = "sys/module/nvidia";
/// PCI vendor ids in `/sys/class/drm/cardN/device/vendor`.
const VENDORS: &[(&str, Gpu)] = &[("0x10de", Gpu::Nvidia), ("0x1002", Gpu::Amd), ("0x8086", Gpu::Intel)];

/// Each GPU's vendor, once per vendor, in card order: the `cardN` entries
/// of the DRM folder. Connectors (`card1-DP-1`) and render nodes have no
/// vendor of their own and are skipped.
pub fn detect(root: &Path) -> Vec<Gpu> {
    let mut cards: Vec<_> = std::fs::read_dir(root.join(DRM_DIR))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let number: u32 = name.strip_prefix("card")?.parse().ok()?;
            Some((number, entry.path()))
        })
        .collect();
    cards.sort();
    let mut found = Vec::new();
    for (_, card) in cards {
        let vendor = std::fs::read_to_string(card.join("device/vendor")).unwrap_or_default();
        if let Some(&(_, gpu)) = VENDORS.iter().find(|(id, _)| *id == vendor.trim())
            && !found.contains(&gpu)
        {
            found.push(gpu);
        }
    }
    found
}

/// NVIDIA's kernel driver is loaded (not nouveau).
pub fn nvidia_driver(root: &Path) -> bool {
    root.join(NVIDIA_MODULE).is_dir()
}
