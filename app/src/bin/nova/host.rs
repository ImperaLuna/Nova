//! The machine Nova runs on: its distro and GPUs. `nova doctor` reports
//! them; `nova setup` picks packages by them.
//!
//! Everything is read below a root folder (`/` for real), so tests run on a
//! fake `/etc` and `/sys`.

use nova_features::{Gpu, gpus};
use std::fmt;
use std::path::Path;

/// os-release, in the order systemd reads it.
const OS_RELEASE: &[&str] = &["etc/os-release", "usr/lib/os-release"];

/// The distro families Nova has package names for, plus the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Distro {
    NixOs,
    Arch,
    Fedora,
    Debian,
    /// Something else, by its os-release `ID` (empty when there's none).
    Other(String),
}

impl Distro {
    /// The key a feature manifest lists this distro's packages under
    /// (`nova_features::DISTROS`). None where Nova has no package names.
    pub fn packages_key(&self) -> Option<&'static str> {
        match self {
            Self::NixOs => Some("nix"),
            Self::Arch => Some("arch"),
            Self::Fedora => Some("fedora"),
            Self::Debian => Some("debian"),
            Self::Other(_) => None,
        }
    }

    /// Reads os-release: `ID`, else the first family in `ID_LIKE` Nova
    /// knows, so Manjaro is Arch and Ubuntu is Debian.
    fn from_os_release(text: &str) -> Self {
        let field = |name: &str| novad::checks::os_release_field(text, name).unwrap_or_default();
        let id = field("ID");
        let like = field("ID_LIKE");
        std::iter::once(id.as_str()).chain(like.split_whitespace()).find_map(Self::known).unwrap_or(Self::Other(id))
    }

    fn known(id: &str) -> Option<Self> {
        match id {
            "nixos" => Some(Self::NixOs),
            "arch" => Some(Self::Arch),
            "fedora" => Some(Self::Fedora),
            "debian" | "ubuntu" => Some(Self::Debian),
            _ => None,
        }
    }
}

impl fmt::Display for Distro {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NixOs => f.write_str("NixOS"),
            Self::Arch => f.write_str("Arch"),
            Self::Fedora => f.write_str("Fedora"),
            Self::Debian => f.write_str("Debian"),
            Self::Other(id) if id.is_empty() => f.write_str("an unknown distro"),
            Self::Other(id) => write!(f, "{id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub distro: Distro,
    /// Each GPU's vendor, once per vendor, in card order.
    pub gpus: Vec<Gpu>,
    /// NVIDIA's kernel driver is loaded (not nouveau).
    pub nvidia_driver: bool,
}

impl Host {
    /// The machine whose files are below `root`.
    pub fn detect(root: &Path) -> Self {
        let os_release = OS_RELEASE.iter().find_map(|path| std::fs::read_to_string(root.join(path)).ok());
        Self {
            distro: Distro::from_os_release(&os_release.unwrap_or_default()),
            gpus: gpus::detect(root),
            nvidia_driver: gpus::nvidia_driver(root),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use nova_features::gpus::{DRM_DIR, NVIDIA_MODULE};

    /// A fake root folder with an os-release and GPUs, removed on drop.
    pub struct FakeRoot(pub nova_test_support::TempDir);

    impl FakeRoot {
        pub fn new(name: &str, os_release: &str, cards: &[&str], nvidia_driver: bool) -> Self {
            let root = nova_test_support::TempDir::new(&format!("host-{name}"));
            std::fs::create_dir_all(root.join("etc")).unwrap();
            std::fs::write(root.join("etc/os-release"), os_release).unwrap();
            for (number, vendor) in cards.iter().enumerate() {
                let device = root.join(DRM_DIR).join(format!("card{number}/device"));
                std::fs::create_dir_all(&device).unwrap();
                std::fs::write(device.join("vendor"), format!("{vendor}\n")).unwrap();
                std::fs::create_dir_all(root.join(DRM_DIR).join(format!("card{number}-DP-1"))).unwrap();
            }
            std::fs::create_dir_all(root.join(DRM_DIR).join("renderD128")).unwrap();
            if nvidia_driver {
                std::fs::create_dir_all(root.join(NVIDIA_MODULE)).unwrap();
            }
            Self(root)
        }
    }

    #[test]
    fn distros_are_read_from_id_then_id_like() {
        let distro = Distro::from_os_release;
        assert_eq!(distro("NAME=NixOS\nID=nixos\n"), Distro::NixOs);
        assert_eq!(distro("ID=arch\n"), Distro::Arch);
        assert_eq!(distro("ID=manjaro\nID_LIKE=arch\n"), Distro::Arch);
        assert_eq!(distro("ID=cachyos\nID_LIKE=\"arch\"\n"), Distro::Arch);
        assert_eq!(distro("ID=\"fedora\"\n"), Distro::Fedora);
        assert_eq!(distro("ID=nobara\nID_LIKE=\"rhel centos fedora\"\n"), Distro::Fedora);
        assert_eq!(distro("ID=pop\nID_LIKE=\"ubuntu debian\"\n"), Distro::Debian);
        assert_eq!(distro("ID=gentoo\n"), Distro::Other("gentoo".into()));
        assert_eq!(distro(""), Distro::Other(String::new()));
        assert_eq!(Distro::Other("gentoo".into()).packages_key(), None);
        let keys: Vec<_> = [Distro::NixOs, Distro::Arch, Distro::Fedora, Distro::Debian]
            .iter()
            .map(|distro| distro.packages_key().unwrap())
            .collect();
        assert_eq!(keys, nova_features::DISTROS);
    }

    #[test]
    fn a_hybrid_laptop_has_both_gpus() {
        let root = FakeRoot::new("hybrid", "ID=arch\n", &["0x8086", "0x10de"], true);
        let host = Host::detect(&root.0);
        assert_eq!(host, Host { distro: Distro::Arch, gpus: vec![Gpu::Intel, Gpu::Nvidia], nvidia_driver: true });
    }

    #[test]
    fn unknown_vendors_and_missing_files_are_no_gpu() {
        let root = FakeRoot::new("vm", "ID=debian\n", &["0x1af4"], false);
        let host = Host::detect(&root.0);
        assert_eq!(host.gpus, vec![]);
        assert!(!host.nvidia_driver);
        let empty = Host::detect(Path::new("/nonexistent"));
        assert_eq!(empty.distro, Distro::Other(String::new()));
    }

    #[test]
    fn os_release_falls_back_to_usr_lib() {
        let root = FakeRoot::new("usr-lib", "", &[], false);
        std::fs::remove_file(root.0.join("etc/os-release")).unwrap();
        std::fs::create_dir_all(root.0.join("usr/lib")).unwrap();
        std::fs::write(root.0.join("usr/lib/os-release"), "ID=fedora\n").unwrap();
        assert_eq!(Host::detect(&root.0).distro, Distro::Fedora);
    }
}
