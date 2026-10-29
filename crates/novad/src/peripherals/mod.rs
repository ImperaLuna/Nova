//! Wireless devices and their batteries (feature `peripherals`): mice,
//! keyboards, headsets and controllers that report a charge, for the quick
//! settings sidebar.
//!
//! Each source of devices is a module of its own behind [`Source`], run on
//! threads of its own, and sends its whole list to [`Report`] whenever it
//! changes: `upower` follows UPower on the system bus (the kernel's drivers
//! and BlueZ, event-driven), `hidpp` asks Logitech devices over HID++ when
//! the kernel left their receiver to `hid-generic` (every [`hidpp::POLL`],
//! when hidraw nodes come or go and when a device says something),
//! `keychron` listens for the battery report that Keychron keyboards on
//! the community battery firmware send over raw HID (by themselves over
//! their dongle, when asked on their cable), and `headsetcontrol` runs
//! HeadsetControl for the headsets the kernel can't read (every [`headsetcontrol::POLL`] and when a USB device comes or
//! goes). A source that isn't there (no UPower, no HeadsetControl, no
//! access to a receiver's node) just reports nothing.
//!
//! The lists are merged into one [`Event::Peripherals`]: a device two
//! sources report (the same USB vendor and product, and serial when both
//! know it) is listed once, as the source first in [`PRIORITY`] (UPower,
//! the kernel's reading, then HID++ and Keychron's) has it, with the
//! other's battery only when it has none. Changes that come together are
//! published once they settle, and only when the list differs from the
//! last one sent.
//!
//! When a device's battery runs low, `alerts` says so with a notification
//! (feature `notifications`), at 20%, 10% and 5%.
//!
//! Only devices that are connected and on now are listed: one that's off
//! (a headset whose dongle stays plugged in) or not present leaves the
//! list. Devices that report no battery anywhere (a keyboard on stock
//! firmware behind its dongle) aren't listed either: finding them would
//! take a HID enumeration of its own.

mod alerts;
#[cfg(test)]
mod fake_hidraw;
mod headsetcontrol;
mod hidpp;
mod hidraw;
mod hotplug;
mod keychron;
mod sysfs;
#[cfg(test)]
mod tests;
mod upower;

use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use crate::notifications::Notifications;
use crate::session_bus::Bus;
use nova_protocol::{Event, Peripheral, PeripheralLink, PeripheralSource, Peripherals as PeripheralsEvent};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

pub const FEATURE_ID: &str = "peripherals";
const LOG_SOURCE: &str = "peripherals";
/// Changes that come together (a device's charge and its state, a source's
/// first list) are published once they pause this long...
const SETTLE: Duration = Duration::from_millis(250);
/// ...or this long after the first of them, if they never pause.
const MAX_DELAY: Duration = Duration::from_secs(1);
/// The sources, the one whose reading wins first.
const PRIORITY: [PeripheralSource; 4] =
    [PeripheralSource::Upower, PeripheralSource::Hidpp, PeripheralSource::Keychron, PeripheralSource::Headsetcontrol];
/// The most devices one source lists: more than any desk has.
const MAX_DEVICES: usize = 32;
/// Device names are cut to this many characters.
const MAX_NAME_CHARS: usize = 100;

/// Where devices are read from: the machine's, or stand-ins in tests.
#[derive(Clone)]
pub struct Sources {
    /// The bus UPower is on.
    pub upower: Bus,
    /// `/sys`, where a UPower device's connection and hidraw nodes are
    /// looked up.
    pub sys_root: PathBuf,
    /// `/dev`, where hidraw nodes are.
    pub dev: PathBuf,
    /// Opens a hidraw node.
    pub open_hidraw: hidraw::Opener,
    /// Searched for `headsetcontrol`.
    pub path_var: String,
    /// `/dev/bus/usb`, whose device nodes come and go with USB devices.
    pub usb_devices: PathBuf,
}

impl Sources {
    pub fn system(path_var: &str) -> Self {
        Self {
            upower: Bus::System,
            sys_root: "/sys".into(),
            dev: "/dev".into(),
            open_hidraw: Arc::new(hidraw::open_node),
            path_var: path_var.into(),
            usb_devices: "/dev/bus/usb".into(),
        }
    }
}

/// A USB vendor and product id, like `1038:2253`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsbId {
    pub vendor: u16,
    pub product: u16,
}

impl std::fmt::Display for UsbId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04x}:{:04x}", self.vendor, self.product)
    }
}

/// A device as one source found it.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub device: Peripheral,
    /// The USB ids it's reached through, to know it when another source
    /// reports it too.
    pub usb_ids: Vec<UsbId>,
    pub serial: Option<String>,
}

impl Found {
    /// Whether `other`, from another source, is this device.
    fn same_as(&self, other: &Found) -> bool {
        let serials_differ = matches!((&self.serial, &other.serial), (Some(mine), Some(theirs)) if mine != theirs);
        self.device.source != other.device.source
            && !serials_differ
            && self.usb_ids.iter().any(|id| other.usb_ids.contains(id))
    }
}

/// A source of devices.
trait Source {
    /// Starts following devices on threads of its own, sending each whole
    /// list to `report`, until the returned handle stops it.
    fn start(self: Box<Self>, report: Report) -> Box<dyn Running>;
}

/// A started [`Source`].
trait Running: Send + Sync {
    /// Ends it; it reports nothing after this.
    fn stop(&self);
}

/// Where one source sends what it found.
#[derive(Clone)]
pub struct Report {
    shared: Arc<Shared>,
    source: PeripheralSource,
}

impl Report {
    /// `devices` is all the source has now.
    fn found(&self, mut devices: Vec<Found>) {
        devices.sort_by(|a, b| a.device.id.cmp(&b.device.id));
        if devices.len() > MAX_DEVICES {
            self.shared.logger.warn(LOG_SOURCE, &format!("left devices out: more than {MAX_DEVICES} reported"));
            devices.truncate(MAX_DEVICES);
        }
        let mut state = lock(&self.shared.state);
        if state.stopped {
            return;
        }
        let Some(slot) = state.found.iter_mut().find(|(source, _)| *source == self.source) else { return };
        if slot.1 == devices {
            return;
        }
        slot.1 = devices;
        let now = Instant::now();
        state.pending = Some(state.pending.map_or((now, now), |(first, _)| (first, now)));
        self.shared.wake.notify_all();
    }

    fn logger(&self) -> &Logger {
        &self.shared.logger
    }
}

pub struct Peripherals {
    shared: Arc<Shared>,
    running: Vec<Box<dyn Running>>,
}

struct Shared {
    state: Mutex<State>,
    /// Wakes the publisher when a source reported, or it should stop.
    wake: Condvar,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    /// Where low battery alerts go, while notifications are on.
    notifications: Mutex<Weak<Notifications>>,
}

struct State {
    /// Each source's devices, in [`PRIORITY`] order.
    found: Vec<(PeripheralSource, Vec<Found>)>,
    /// The last event published.
    sent: PeripheralsEvent,
    /// When the first and the last change not yet published came.
    pending: Option<(Instant, Instant)>,
    stopped: bool,
    alerts: alerts::Alerts,
}

impl Peripherals {
    /// Starts every source and the publisher. Returns at once: devices are
    /// published as the sources find them.
    pub fn start(sources: Sources, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                found: PRIORITY.iter().map(|source| (*source, Vec::new())).collect(),
                sent: PeripheralsEvent { devices: Vec::new() },
                pending: None,
                stopped: false,
                alerts: alerts::Alerts::default(),
            }),
            wake: Condvar::new(),
            hub,
            logger,
            notifications: Mutex::new(Weak::new()),
        });
        // Clients that heard an earlier run's list (the feature was off a
        // while) start from nothing again, even if nothing is found now.
        shared.hub.publish(&Event::Peripherals(PeripheralsEvent { devices: Vec::new() }));
        let publisher = Arc::clone(&shared);
        std::thread::spawn(move || publisher.publish_when_settled());
        let started: [(PeripheralSource, Box<dyn Source>); 4] = [
            (
                PeripheralSource::Upower,
                Box::new(upower::UPower { bus: sources.upower, sys_root: sources.sys_root.clone() }),
            ),
            (
                PeripheralSource::Hidpp,
                Box::new(hidraw::Hidraw {
                    sys_root: sources.sys_root.clone(),
                    dev: sources.dev.clone(),
                    open: Arc::clone(&sources.open_hidraw),
                    protocol: hidpp::Hidpp,
                }),
            ),
            (
                PeripheralSource::Keychron,
                Box::new(hidraw::Hidraw {
                    sys_root: sources.sys_root,
                    dev: sources.dev,
                    open: sources.open_hidraw,
                    protocol: keychron::Keychron,
                }),
            ),
            (
                PeripheralSource::Headsetcontrol,
                Box::new(headsetcontrol::HeadsetControl {
                    path_var: sources.path_var,
                    usb_devices: sources.usb_devices,
                }),
            ),
        ];
        let running = started
            .into_iter()
            .map(|(source, started)| started.start(Report { shared: Arc::clone(&shared), source }))
            .collect();
        Arc::new(Self { shared, running })
    }

    /// Stops the sources and publishes nothing more.
    pub fn stop(&self) {
        lock(&self.shared.state).stopped = true;
        self.shared.wake.notify_all();
        for running in &self.running {
            running.stop();
        }
    }

    /// Where low battery alerts go from now on: nowhere while
    /// notifications are off.
    pub fn set_notifications(&self, notifications: Option<&Arc<Notifications>>) {
        *lock(&self.shared.notifications) = notifications.map(Arc::downgrade).unwrap_or_default();
    }

    /// Sends a new subscriber, client `id`, the devices last published,
    /// under the lock that publishing takes, so a newer event can't arrive
    /// before it.
    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.shared.state);
        self.shared.hub.send_to(id, &Event::Peripherals(state.sent.clone()));
    }
}

impl Shared {
    /// Publishes the merged list once changes settle, until stopped.
    fn publish_when_settled(&self) {
        let mut state = lock(&self.state);
        loop {
            if state.stopped {
                return;
            }
            let Some((first, last)) = state.pending else {
                state = self.wake.wait(state).unwrap_or_else(std::sync::PoisonError::into_inner);
                continue;
            };
            let due = (last + SETTLE).min(first + MAX_DELAY);
            let now = Instant::now();
            if now < due {
                state = self.wake.wait_timeout(state, due - now).unwrap_or_else(std::sync::PoisonError::into_inner).0;
                continue;
            }
            state.pending = None;
            let event = merge(&state.found);
            if event != state.sent {
                self.hub.publish(&Event::Peripherals(event.clone()));
                // Only while notifications are on: alerts nobody can see
                // aren't counted as said.
                let notifications = lock(&self.notifications).upgrade();
                let alerts = match &notifications {
                    Some(_) => state.alerts.update(&event.devices),
                    None => Vec::new(),
                };
                state.sent = event;
                if let Some(notifications) = notifications.filter(|_| !alerts.is_empty()) {
                    // Sent unlocked: the notification server takes locks of its own.
                    drop(state);
                    let sent = self.alert(&notifications, alerts);
                    state = lock(&self.state);
                    for (key, notification) in sent {
                        state.alerts.sent(&key, notification);
                    }
                }
            }
        }
    }

    /// Sends `alerts` and says which notification each became.
    fn alert(&self, notifications: &Notifications, alerts: Vec<alerts::Alert>) -> Vec<(String, u32)> {
        alerts
            .into_iter()
            .map(|alert| {
                self.logger.info(LOG_SOURCE, &alert.own.summary);
                let id = notifications.send_own(alert.own);
                (alert.key, id)
            })
            .collect()
    }
}

/// One list from every source's: each device once, as the first source in
/// [`PRIORITY`] that reports it has it, with a later source's battery when
/// it has none, and a later source's link when the first knows none or
/// only says wired.
fn merge(found: &[(PeripheralSource, Vec<Found>)]) -> PeripheralsEvent {
    let mut kept: Vec<&Found> = Vec::new();
    let mut devices: Vec<Peripheral> = Vec::new();
    for candidate in found.iter().flat_map(|(_, list)| list) {
        match kept.iter().position(|known| known.same_as(candidate)) {
            Some(index) => {
                let device = &mut devices[index];
                if device.battery.is_none() && candidate.device.battery.is_some() {
                    device.battery = candidate.device.battery;
                    device.charging = candidate.device.charging;
                }
                // sysfs can't tell a headset's USB dongle from a cable: a
                // source that knows it's wireless says how it connects.
                if matches!(device.link, None | Some(PeripheralLink::Wired)) && candidate.device.link.is_some() {
                    device.link = candidate.device.link;
                }
            }
            None => {
                kept.push(candidate);
                devices.push(candidate.device.clone());
            }
        }
    }
    PeripheralsEvent { devices }
}

/// A device's name from text another program gave: trimmed, cut to
/// [`MAX_NAME_CHARS`], `None` when empty.
fn name(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.chars().filter(|c| !c.is_control()).take(MAX_NAME_CHARS).collect())
}
