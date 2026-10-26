//! Which EGL vendors the UI loads. glvnd loads every vendor it finds, so on
//! a machine whose only GPU is NVIDIA's, with NVIDIA's driver, the UI also
//! mapped Mesa's (libEGL_mesa, libgallium, libLLVM): 11 MB Pss it never
//! draws with (IMP-153). There the UI gets NVIDIA's vendor file alone.

use nova_features::Gpu;
use nova_features::gpus;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// glvnd's list of vendor files to load instead of searching its folders.
pub const VENDOR_FILES_VAR: &str = "__EGL_VENDOR_LIBRARY_FILENAMES";
/// glvnd's folders to search instead of its own.
const VENDOR_DIRS_VAR: &str = "__EGL_VENDOR_LIBRARY_DIRS";
/// Where glvnd finds vendor files: its default folders, and NixOS's driver
/// folder, which its glvnd searches instead.
const VENDOR_DIRS: &[&str] =
    &["etc/glvnd/egl_vendor.d", "usr/share/glvnd/egl_vendor.d", "run/opengl-driver/share/glvnd/egl_vendor.d"];

/// NVIDIA's EGL vendor file when the machine below `root` has only NVIDIA
/// GPUs with NVIDIA's driver, and the session (`env`) doesn't pick vendors
/// itself. None otherwise: a laptop with another GPU may draw on that one.
pub fn nvidia_only_vendor(root: &Path, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if env(VENDOR_FILES_VAR).is_some() || env(VENDOR_DIRS_VAR).is_some() {
        return None;
    }
    if gpus::detect(root) != [Gpu::Nvidia] || !gpus::nvidia_driver(root) {
        return None;
    }
    VENDOR_DIRS.iter().find_map(|dir| {
        let mut files: Vec<PathBuf> = std::fs::read_dir(root.join(dir))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|extension| extension == "json")
                    && path.file_name().is_some_and(|name| name.to_string_lossy().contains("nvidia"))
            })
            .collect();
        files.sort();
        files.into_iter().next()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_features::gpus::{DRM_DIR, NVIDIA_MODULE};

    /// A fake root with GPUs of `vendors` and glvnd's NixOS folder holding
    /// NVIDIA's and Mesa's vendor files.
    fn root(name: &str, vendors: &[&str], nvidia_driver: bool) -> PathBuf {
        let root = std::env::temp_dir().join(format!("novad-egl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (number, vendor) in vendors.iter().enumerate() {
            let device = root.join(DRM_DIR).join(format!("card{number}/device"));
            std::fs::create_dir_all(&device).unwrap();
            std::fs::write(device.join("vendor"), format!("{vendor}\n")).unwrap();
        }
        if nvidia_driver {
            std::fs::create_dir_all(root.join(NVIDIA_MODULE)).unwrap();
        }
        let vendors = root.join(VENDOR_DIRS[2]);
        std::fs::create_dir_all(&vendors).unwrap();
        std::fs::write(vendors.join("10_nvidia.json"), "{}").unwrap();
        std::fs::write(vendors.join("50_mesa.json"), "{}").unwrap();
        root
    }

    #[test]
    fn only_nvidia_with_its_driver_gets_its_vendor_alone() {
        let no_env = |_: &str| None;
        let nvidia = root("nvidia", &["0x10de"], true);
        assert_eq!(nvidia_only_vendor(&nvidia, no_env), Some(nvidia.join(VENDOR_DIRS[2]).join("10_nvidia.json")));
        // The session picks vendors itself.
        let set = |name: &str| (name == VENDOR_DIRS_VAR).then(|| OsString::from("/somewhere"));
        assert_eq!(nvidia_only_vendor(&nvidia, set), None);

        let hybrid = root("hybrid", &["0x8086", "0x10de"], true);
        assert_eq!(nvidia_only_vendor(&hybrid, no_env), None);
        let nouveau = root("nouveau", &["0x10de"], false);
        assert_eq!(nvidia_only_vendor(&nouveau, no_env), None);
        let amd = root("amd", &["0x1002"], false);
        assert_eq!(nvidia_only_vendor(&amd, no_env), None);
        for dir in [nvidia, hybrid, nouveau, amd] {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
