use crate::Gpu;
use std::path::Path;

pub const DRM_DIR: &str = "sys/class/drm";
pub const NVIDIA_MODULE: &str = "sys/module/nvidia";
const VENDORS: &[(&str, Gpu)] = &[("0x10de", Gpu::Nvidia), ("0x1002", Gpu::Amd), ("0x8086", Gpu::Intel)];

pub fn detect(root: &Path) -> Vec<Gpu> {
    let mut found = Vec::new();
    for gpu in card_vendors(root).into_iter().flatten() {
        if !found.contains(&gpu) {
            found.push(gpu);
        }
    }
    found
}

pub fn only_nvidia(root: &Path) -> bool {
    let cards = card_vendors(root);
    !cards.is_empty() && cards.iter().all(|gpu| *gpu == Some(Gpu::Nvidia))
}

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

pub fn nvidia_driver(root: &Path) -> bool {
    root.join(NVIDIA_MODULE).is_dir()
}
