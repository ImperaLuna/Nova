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

/// Each GPU's vendor, once per vendor, in card order. Vendors Nova doesn't
/// know (a VM's virtio GPU) are left out.
pub fn detect(root: &Path) -> Vec<Gpu> {
    let mut found = Vec::new();
    for gpu in card_vendors(root).into_iter().flatten() {
        if !found.contains(&gpu) {
            found.push(gpu);
        }
    }
    found
}

/// There's a GPU, and every one is NVIDIA's: none Nova doesn't know.
pub fn only_nvidia(root: &Path) -> bool {
    let cards = card_vendors(root);
    !cards.is_empty() && cards.iter().all(|gpu| *gpu == Some(Gpu::Nvidia))
}

/// Each `cardN` entry's vendor in the DRM folder, in card order (None for
/// one Nova doesn't know). Connectors (`card1-DP-1`) and render nodes have
/// no vendor of their own and are skipped.
fn card_vendors(root: &Path) -> Vec<Option<Gpu>> {
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
    cards
        .into_iter()
        .map(|(_, card)| {
            let vendor = std::fs::read_to_string(card.join("device/vendor")).unwrap_or_default();
            VENDORS.iter().find(|(id, _)| *id == vendor.trim()).map(|&(_, gpu)| gpu)
        })
        .collect()
}

/// NVIDIA's kernel driver is loaded (not nouveau).
pub fn nvidia_driver(root: &Path) -> bool {
    root.join(NVIDIA_MODULE).is_dir()
}
