use std::path::{Path, PathBuf};

const CPU_SENSORS: [(&str, Option<&str>); 5] = [
    ("k10temp", Some("Tctl")),
    ("zenpower", Some("Tdie")),
    ("coretemp", Some("Package id 0")),
    ("cpu_thermal", None),
    ("acpitz", None),
];
const MILLIDEGREES: f32 = 1000.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Temperature(PathBuf);

impl Temperature {
    pub fn find(sys_root: &Path) -> Option<Self> {
        let devices: Vec<PathBuf> = std::fs::read_dir(sys_root.join("class/hwmon"))
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        let name =
            |device: &Path| std::fs::read_to_string(device.join("name")).ok().map(|name| name.trim().to_string());
        CPU_SENSORS.iter().find_map(|(driver, label)| {
            let device = devices.iter().find(|device| name(device).as_deref() == Some(*driver))?;
            input_for(device, *label).map(Self)
        })
    }

    pub fn read(&self) -> Option<f32> {
        let millidegrees: f32 = std::fs::read_to_string(&self.0).ok()?.trim().parse().ok()?;
        Some(millidegrees / MILLIDEGREES)
    }
}

fn input_for(device: &Path, label: Option<&str>) -> Option<PathBuf> {
    let mut inputs: Vec<PathBuf> = std::fs::read_dir(device)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("temp") && name.ends_with("_input"))
        })
        .collect();
    inputs.sort();
    let labelled = label.and_then(|label| {
        inputs.iter().find(|input| {
            let label_file = input.to_string_lossy().replace("_input", "_label");
            std::fs::read_to_string(label_file).is_ok_and(|text| text.trim() == label)
        })
    });
    labelled.or(inputs.first()).cloned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuBusy(PathBuf);

impl GpuBusy {
    pub fn find(sys_root: &Path) -> Option<Self> {
        let mut cards: Vec<PathBuf> = std::fs::read_dir(sys_root.join("class/drm"))
            .ok()?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.strip_prefix("card"))
                    .is_some_and(|number| !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
            })
            .map(|entry| entry.path().join("device/gpu_busy_percent"))
            .filter(|path| path.is_file())
            .collect();
        cards.sort();
        cards.into_iter().next().map(Self)
    }

    pub fn read(&self) -> Option<f32> {
        std::fs::read_to_string(&self.0).ok()?.trim().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sysfs(name: &str, files: &[(&str, &str)]) -> nova_test_support::TempDir {
        let root = nova_test_support::TempDir::new(&format!("sensors-{name}"));
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        root
    }

    #[test]
    fn picks_the_cpu_package_sensor() {
        let root = sysfs(
            "amd",
            &[
                ("class/hwmon/hwmon0/name", "nvme\n"),
                ("class/hwmon/hwmon0/temp1_input", "38850\n"),
                ("class/hwmon/hwmon1/name", "amdgpu\n"),
                ("class/hwmon/hwmon1/temp1_input", "45000\n"),
                ("class/hwmon/hwmon2/name", "k10temp\n"),
                ("class/hwmon/hwmon2/temp1_input", "52125\n"),
                ("class/hwmon/hwmon2/temp1_label", "Tctl\n"),
                ("class/hwmon/hwmon2/temp3_input", "47000\n"),
                ("class/hwmon/hwmon2/temp3_label", "Tccd1\n"),
            ],
        );
        let sensor = Temperature::find(&root).unwrap();
        assert_eq!(sensor.read(), Some(52.125));

        let root = sysfs(
            "intel",
            &[
                ("class/hwmon/hwmon3/name", "acpitz\n"),
                ("class/hwmon/hwmon3/temp1_input", "27800\n"),
                ("class/hwmon/hwmon5/name", "coretemp\n"),
                ("class/hwmon/hwmon5/temp1_input", "61000\n"),
                ("class/hwmon/hwmon5/temp1_label", "Core 0\n"),
                ("class/hwmon/hwmon5/temp2_input", "64000\n"),
                ("class/hwmon/hwmon5/temp2_label", "Package id 0\n"),
            ],
        );
        assert_eq!(Temperature::find(&root).and_then(|sensor| sensor.read()), Some(64.0));
    }

    #[test]
    fn no_cpu_sensor_is_none() {
        let root = sysfs("none", &[("class/hwmon/hwmon0/name", "nvme\n"), ("class/hwmon/hwmon0/temp1_input", "1\n")]);
        assert_eq!(Temperature::find(&root), None);
        assert_eq!(Temperature::find(Path::new("/nonexistent")), None);
    }

    #[test]
    fn gpu_busy_from_the_first_card_that_reports_it() {
        let root = sysfs(
            "gpu",
            &[
                ("class/drm/card0-DP-1/status", "connected\n"),
                ("class/drm/card0/device/vendor", "0x8086\n"),
                ("class/drm/card1/device/gpu_busy_percent", "18\n"),
            ],
        );
        assert_eq!(GpuBusy::find(&root).and_then(|gpu| gpu.read()), Some(18.0));
        assert_eq!(GpuBusy::find(Path::new("/nonexistent")), None);
    }
}
