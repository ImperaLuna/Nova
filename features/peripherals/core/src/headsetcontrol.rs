use super::hotplug::{self, Waker};
use super::{Found, LOG_SOURCE, Report, Running, Source, UsbId, name};
use nova_carrier::log::LogLevel;
use nova_inotify as inotify;
use nova_peripherals_api::{Battery, Peripheral, PeripheralKind, PeripheralLink, PeripheralSource};
use nova_process::child;
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

const BINARY: &str = "headsetcontrol";
pub const POLL: Duration = Duration::from_secs(60);
pub const HOTPLUG_SETTLE: Duration = Duration::from_secs(2);
const RUN_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const ID_PREFIX: &str = "headsetcontrol/";
const STATUS_SUCCESS: &str = "success";
const CAP_BATTERY: &str = "CAP_BATTERY_STATUS";
const BATTERY_AVAILABLE: &str = "BATTERY_AVAILABLE";
const BATTERY_CHARGING: &str = "BATTERY_CHARGING";

pub struct HeadsetControl {
    pub path_var: String,
    pub usb_devices: PathBuf,
}

struct Reading {
    waker: Arc<Waker>,
    hotplug: Option<inotify::Stopper>,
}

impl Running for Reading {
    fn stop(&self) {
        self.waker.stop();
        if let Some(hotplug) = &self.hotplug {
            hotplug.stop();
        }
    }
}

impl Source for HeadsetControl {
    fn start(self: Box<Self>, report: Report) -> Box<dyn Running> {
        let waker = Arc::new(Waker::default());
        let plugged = Arc::clone(&waker);
        let failed = report.clone();
        let hotplug = hotplug::watch(
            &self.usb_devices,
            move || plugged.plugged(),
            move |e| failed.logger().warn(LOG_SOURCE, &format!("stopped following USB devices: {e}")),
        )
        .map_err(|e| {
            let problem = format!("can't follow USB devices in {} ({e})", self.usb_devices.display());
            report.logger().log(LogLevel::Debug, LOG_SOURCE, &format!("{problem}; headsets are read every minute"));
        })
        .ok();
        let reader = Arc::clone(&waker);
        std::thread::spawn(move || self.read_until_stopped(&reader, &report));
        Box::new(Reading { waker, hotplug })
    }
}

impl HeadsetControl {
    fn read_until_stopped(&self, waker: &Waker, report: &Report) {
        let mut runs = Runs::default();
        let mut next_poll = Instant::now();
        loop {
            if !waker.wait(next_poll, HOTPLUG_SETTLE) {
                return;
            }
            next_poll = Instant::now() + POLL;
            let found = runs.read(&self.path_var, report);
            report.found(found);
        }
    }
}

#[derive(Default)]
struct Runs {
    installed: Option<bool>,
    problem: Option<String>,
}

impl Runs {
    fn read(&mut self, path_var: &str, report: &Report) -> Vec<Found> {
        let binary = nova_paths::find_program(BINARY, path_var);
        if self.installed != Some(binary.is_some()) {
            let said = if binary.is_some() { "found" } else { "not installed, so no headsets from it" };
            report.logger().info(LOG_SOURCE, &format!("HeadsetControl {said}"));
            self.installed = Some(binary.is_some());
        }
        let Some(binary) = binary else { return Vec::new() };
        let mut command = Command::new(binary);
        command.args(["-o", "json"]).env_clear();
        let answer = child::output_within(command, RUN_TIMEOUT)
            .map_err(|e| format!("didn't answer: {e}"))
            .and_then(|output| parse(&output.stdout).map_err(|e| format!("{e} ({})", output.status)));
        match answer {
            Ok(found) => {
                if self.problem.take().is_some() {
                    report.logger().info(LOG_SOURCE, "HeadsetControl answers again");
                }
                found
            }
            Err(problem) => {
                if self.problem.as_ref() != Some(&problem) {
                    report.logger().warn(LOG_SOURCE, &format!("HeadsetControl {problem}"));
                    self.problem = Some(problem);
                }
                Vec::new()
            }
        }
    }
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    devices: Vec<Device>,
}

#[derive(Deserialize)]
struct Device {
    #[serde(default)]
    status: String,
    #[serde(default)]
    device: String,
    #[serde(default)]
    product: String,
    #[serde(default)]
    vendor: String,
    #[serde(default)]
    id_vendor: serde_json::Value,
    #[serde(default)]
    id_product: serde_json::Value,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    battery: Option<BatteryStatus>,
}

#[derive(Deserialize)]
struct BatteryStatus {
    #[serde(default)]
    status: String,
    #[serde(default)]
    level: Option<i64>,
}

pub fn parse(output: &[u8]) -> Result<Vec<Found>, String> {
    if output.len() > MAX_OUTPUT_BYTES {
        return Err(format!("answered more than {MAX_OUTPUT_BYTES} bytes"));
    }
    let answer: Answer =
        serde_json::from_slice(output).map_err(|e| format!("answered something else than JSON: {e}"))?;
    Ok(answer.devices.iter().filter_map(found).collect())
}

fn found(device: &Device) -> Option<Found> {
    if device.status != STATUS_SUCCESS || !device.capabilities.iter().any(|capability| capability == CAP_BATTERY) {
        return None;
    }
    let id = UsbId { vendor: hex(&device.id_vendor)?, product: hex(&device.id_product)? };
    let status = device.battery.as_ref().map(|battery| battery.status.as_str()).unwrap_or_default();
    if status != BATTERY_AVAILABLE && status != BATTERY_CHARGING {
        return None;
    }
    let level = device.battery.as_ref().and_then(|battery| battery.level).filter(|level| (0..=100).contains(level));
    let battery = level.and_then(|level| u8::try_from(level).ok()).map(Battery::Percent);
    let device = Peripheral {
        id: format!("{ID_PREFIX}{id}"),
        name: name(without_vendor(&device.product, &device.vendor))
            .or_else(|| name(&device.device))
            .unwrap_or_else(|| id.to_string()),
        kind: PeripheralKind::Headset,
        link: Some(PeripheralLink::Dongle),
        battery,
        charging: status == BATTERY_CHARGING,
        source: PeripheralSource::Headsetcontrol,
    };
    Some(Found { device, usb_ids: vec![id], serial: None })
}

fn without_vendor<'a>(product: &'a str, vendor: &str) -> &'a str {
    let product = product.trim();
    let vendor = vendor.trim();
    match product.strip_prefix(vendor) {
        Some(rest) if !vendor.is_empty() && rest.starts_with(char::is_whitespace) && !rest.trim().is_empty() => {
            rest.trim_start()
        }
        _ => product,
    }
}

fn hex(value: &serde_json::Value) -> Option<u16> {
    if let Some(number) = value.as_u64() {
        return u16::try_from(number).ok();
    }
    let digits = value.as_str()?.trim();
    let digits = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")).unwrap_or(digits);
    u16::from_str_radix(digits, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::without_vendor;

    #[test]
    fn only_the_makers_whole_name_comes_off() {
        assert_eq!(without_vendor("SteelSeries Arctis Nova 5X", "SteelSeries"), "Arctis Nova 5X");
        assert_eq!(without_vendor("Logitech G733", "Logi"), "Logitech G733");
        assert_eq!(without_vendor("HyperX", "HyperX"), "HyperX");
        assert_eq!(without_vendor("Arctis 7", ""), "Arctis 7");
    }
}
