//! Headsets the kernel can't read the battery of (SteelSeries', HyperX's,
//! Corsair's on their USB dongles), from HeadsetControl: `headsetcontrol -o
//! json` is run, never linked (it's GPL-3), every [`POLL`] and shortly
//! after a USB device is plugged in or out (`hotplug`), with a timeout. It
//! prints every headset it supports, with its dongle's USB ids and its
//! battery. Without `headsetcontrol` on PATH (looked for at each run, so
//! installing it later works) this source lists nothing.
//!
//! It exits 1 when no headset is plugged in, still printing its JSON, so
//! the output is read whatever the exit status. Unknown fields are ignored:
//! newer releases add some.

use super::{Found, LOG_SOURCE, Report, Running, Source, UsbId, hotplug, name};
use crate::child;
use crate::inotify;
use crate::lock;
use nova_protocol::{Battery, LogLevel, Peripheral, PeripheralKind, PeripheralLink, PeripheralSource};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const BINARY: &str = "headsetcontrol";
/// How often headsets are read: a headset turned on or off doesn't plug
/// anything in, so only a look tells.
pub const POLL: Duration = Duration::from_secs(60);
/// How long after a USB device comes or goes headsets are read: a dongle
/// needs a moment before its HID interface opens.
pub const HOTPLUG_SETTLE: Duration = Duration::from_secs(2);
/// How long `headsetcontrol` may take to answer.
const RUN_TIMEOUT: Duration = Duration::from_secs(10);
/// More than its answer for any desk of headsets.
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
/// The id a headset's comes after, before its dongle's USB ids.
const ID_PREFIX: &str = "headsetcontrol/";
/// A device entry HeadsetControl could read.
const STATUS_SUCCESS: &str = "success";
/// The capability of reporting a battery.
const CAP_BATTERY: &str = "CAP_BATTERY_STATUS";
const BATTERY_AVAILABLE: &str = "BATTERY_AVAILABLE";
const BATTERY_CHARGING: &str = "BATTERY_CHARGING";

pub struct HeadsetControl {
    pub path_var: String,
    pub usb_devices: PathBuf,
}

/// What wakes the reader.
#[derive(Default)]
struct Wakes {
    stopped: bool,
    /// When a USB device last came or went, until headsets are read.
    plugged: Option<Instant>,
}

#[derive(Default)]
struct Waker {
    wakes: Mutex<Wakes>,
    condvar: Condvar,
}

struct Reading {
    waker: Arc<Waker>,
    hotplug: Option<inotify::Stopper>,
}

impl Running for Reading {
    fn stop(&self) {
        lock(&self.waker.wakes).stopped = true;
        self.waker.condvar.notify_all();
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
            move || {
                lock(&plugged.wakes).plugged = Some(Instant::now());
                plugged.condvar.notify_all();
            },
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
    /// Reads headsets now, then every [`POLL`] and after USB devices
    /// change, until stopped.
    fn read_until_stopped(&self, waker: &Waker, report: &Report) {
        let mut runs = Runs::default();
        let mut next_poll = Instant::now();
        loop {
            {
                let mut wakes = lock(&waker.wakes);
                loop {
                    if wakes.stopped {
                        return;
                    }
                    let due = wakes.plugged.map_or(next_poll, |plugged| next_poll.min(plugged + HOTPLUG_SETTLE));
                    let now = Instant::now();
                    if now >= due {
                        break;
                    }
                    wakes = waker
                        .condvar
                        .wait_timeout(wakes, due - now)
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .0;
                }
                wakes.plugged = None;
            }
            next_poll = Instant::now() + POLL;
            let found = runs.read(&self.path_var, report);
            report.found(found);
        }
    }
}

/// What the last runs said, so a lasting problem is logged once.
#[derive(Default)]
struct Runs {
    installed: Option<bool>,
    problem: Option<String>,
}

impl Runs {
    /// The headsets HeadsetControl sees now; none when it isn't installed
    /// or fails (logged when that changes).
    fn read(&mut self, path_var: &str, report: &Report) -> Vec<Found> {
        let binary = nova_paths::find_program(BINARY, path_var);
        if self.installed != Some(binary.is_some()) {
            let said = if binary.is_some() { "found" } else { "not installed, so no headsets from it" };
            report.logger().info(LOG_SOURCE, &format!("HeadsetControl {said}"));
            self.installed = Some(binary.is_some());
        }
        let Some(binary) = binary else { return Vec::new() };
        let mut command = Command::new(binary);
        // It needs nothing from novad's environment.
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

/// `headsetcontrol -o json`'s answer.
#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    devices: Vec<Device>,
}

#[derive(Deserialize)]
struct Device {
    #[serde(default)]
    status: String,
    /// Its family's name (`SteelSeries Arctis Nova (5/5X)`).
    #[serde(default)]
    device: String,
    /// The model itself (`SteelSeries Arctis Nova 5X`).
    #[serde(default)]
    product: String,
    /// Its dongle's USB ids, as hex (`"0x1038"`), or a number.
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
    /// -1 while unknown.
    #[serde(default)]
    level: Option<i64>,
}

/// The headsets in `headsetcontrol -o json`'s answer that are on and report
/// a battery. Entries it couldn't read (`"status": "failure"`) and headsets
/// that are off are left out.
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
    // Off, or out of reach (`BATTERY_UNAVAILABLE`): its dongle answers for
    // it, but the headset isn't there.
    if status != BATTERY_AVAILABLE && status != BATTERY_CHARGING {
        return None;
    }
    let level = device.battery.as_ref().and_then(|battery| battery.level).filter(|level| (0..=100).contains(level));
    let battery = level.and_then(|level| u8::try_from(level).ok()).map(Battery::Percent);
    let device = Peripheral {
        id: format!("{ID_PREFIX}{id}"),
        name: name(&device.product).or_else(|| name(&device.device)).unwrap_or_else(|| id.to_string()),
        kind: PeripheralKind::Headset,
        // HeadsetControl reaches headsets through their USB dongle.
        link: Some(PeripheralLink::Dongle),
        battery,
        charging: status == BATTERY_CHARGING,
        source: PeripheralSource::Headsetcontrol,
    };
    Some(Found { device, usb_ids: vec![id], serial: None })
}

/// A USB id: text in hex, with or without `0x`, or a number.
fn hex(value: &serde_json::Value) -> Option<u16> {
    if let Some(number) = value.as_u64() {
        return u16::try_from(number).ok();
    }
    let digits = value.as_str()?.trim();
    let digits = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")).unwrap_or(digits);
    u16::from_str_radix(digits, 16).ok()
}
