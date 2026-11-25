mod alerts;
#[cfg(test)]
mod fake_hidraw;
mod headsetcontrol;
mod hidpp;
mod hidraw;
mod hotplug;
mod keychron;
mod keychron_mouse;
mod sysfs;
#[cfg(test)]
mod tests;
mod upower;

use nova_base::lock;
use nova_hub::{ClientId, Hub};
use nova_log::Logger;
use nova_notifications_api::Post;
use nova_peripherals_api::{
    Event, Peripheral, PeripheralKind, PeripheralLink, PeripheralSource, Peripherals as PeripheralsEvent,
};
use nova_session_bus::Bus;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

pub const FEATURE_ID: &str = "peripherals";
const LOG_SOURCE: &str = "peripherals";
const SETTLE: Duration = Duration::from_millis(250);
const MAX_DELAY: Duration = Duration::from_secs(1);
const PRIORITY: [PeripheralSource; 4] =
    [PeripheralSource::Upower, PeripheralSource::Hidpp, PeripheralSource::Keychron, PeripheralSource::Headsetcontrol];
const MAX_DEVICES: usize = 32;
const MAX_NAME_CHARS: usize = 100;

#[derive(Clone)]
pub struct Sources {
    pub upower: Bus,
    pub sys_root: PathBuf,
    pub dev: PathBuf,
    pub open_hidraw: hidraw::Opener,
    pub path_var: String,
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

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub device: Peripheral,
    pub usb_ids: Vec<UsbId>,
    pub serial: Option<String>,
}

impl Found {
    fn same_as(&self, other: &Found) -> bool {
        let serials_differ = matches!((&self.serial, &other.serial), (Some(mine), Some(theirs)) if mine != theirs);
        self.device.source != other.device.source
            && !serials_differ
            && self.usb_ids.iter().any(|id| other.usb_ids.contains(id))
    }

    fn same_over_bluetooth(&self, other: &Found) -> bool {
        let bluetooth = |found: &Found| found.device.link == Some(PeripheralLink::Bluetooth);
        bluetooth(self) != bluetooth(other)
            && same_kind(self.device.kind, other.device.kind)
            && names_match(&self.device.name, &other.device.name)
    }
}

fn same_kind(a: PeripheralKind, b: PeripheralKind) -> bool {
    let audio = |kind| matches!(kind, PeripheralKind::Headset | PeripheralKind::Headphones);
    a == b || (audio(a) && audio(b))
}

fn names_match(a: &str, b: &str) -> bool {
    let words = |name: &str| name.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>();
    let (a, b) = (words(a), words(b));
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    !short.is_empty() && long.windows(short.len()).any(|run| run == short.as_slice())
}

trait Source {
    fn start(self: Box<Self>, report: Report) -> Box<dyn Running>;
}

trait Running: Send + Sync {
    fn stop(&self);
}

#[derive(Clone)]
pub struct Report {
    shared: Arc<Shared>,
    source: PeripheralSource,
}

impl Report {
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
    wake: Condvar,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    notifications: Mutex<Option<Weak<dyn Post>>>,
}

struct State {
    found: Vec<(PeripheralSource, Vec<Found>)>,
    sent: PeripheralsEvent,
    pending: Option<(Instant, Instant)>,
    stopped: bool,
    alerts: alerts::Alerts,
}

impl Peripherals {
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
            notifications: Mutex::new(None),
        });
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

    pub fn stop(&self) {
        lock(&self.shared.state).stopped = true;
        self.shared.wake.notify_all();
        for running in &self.running {
            running.stop();
        }
    }

    pub fn set_notifications(&self, notifications: Option<&Arc<dyn Post>>) {
        let notifications = notifications.map(Arc::downgrade);
        let changed = match (&*lock(&self.shared.notifications), &notifications) {
            (Some(was), Some(now)) => !was.ptr_eq(now),
            (was, now) => was.is_some() != now.is_some(),
        };
        *lock(&self.shared.notifications) = notifications;
        if changed {
            let mut state = lock(&self.shared.state);
            let now = Instant::now();
            state.pending = Some(state.pending.map_or((now, now), |(first, _)| (first, now)));
            self.shared.wake.notify_all();
        }
    }

    pub fn send_state(&self, id: ClientId) {
        let state = lock(&self.shared.state);
        self.shared.hub.send_to(id, &Event::Peripherals(state.sent.clone()));
    }
}

impl Shared {
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
                state.sent = event;
            }
            let Some(notifications) = lock(&self.notifications).as_ref().and_then(Weak::upgrade) else { continue };
            let State { alerts, sent, .. } = &mut *state;
            let alerts = alerts.update(&sent.devices);
            if !alerts.is_empty() {
                let held = state.alerts.notifications();
                drop(state);
                let sent = self.alert(&*notifications, alerts, held);
                state = lock(&self.state);
                for (id, notification) in sent {
                    state.alerts.sent(&id, notification);
                }
            }
        }
    }

    fn alert(&self, notifications: &dyn Post, alerts: Vec<alerts::Alert>, mut held: Vec<u32>) -> Vec<(String, u32)> {
        alerts
            .into_iter()
            .take_while(|_| !lock(&self.state).stopped)
            .map(|mut alert| {
                if alert.own.replaces == 0 {
                    let row = notifications.listed(&|row| !held.contains(&row.id) && alerts::row_of(row, &alert.own));
                    alert.own.replaces = row.unwrap_or(0);
                }
                self.logger.info(LOG_SOURCE, &format!("{}: {}", alert.own.summary, alert.own.body));
                let id = notifications.post(alert.own);
                held.push(id);
                (alert.id, id)
            })
            .collect()
    }
}

fn merge(found: &[(PeripheralSource, Vec<Found>)]) -> PeripheralsEvent {
    let mut kept: Vec<&Found> = Vec::new();
    let mut devices: Vec<Peripheral> = Vec::new();
    for candidate in found.iter().flat_map(|(_, list)| list) {
        if let Some(index) = kept.iter().position(|known| known.same_over_bluetooth(candidate))
            && !kept.iter().any(|known| known.same_as(candidate))
            && candidate.device.link != Some(PeripheralLink::Bluetooth)
        {
            let bluetooth = std::mem::replace(&mut devices[index], candidate.device.clone());
            if devices[index].battery.is_none() {
                devices[index].battery = bluetooth.battery;
                devices[index].charging = bluetooth.charging;
            }
            kept[index] = candidate;
            continue;
        }
        let same = kept
            .iter()
            .position(|known| known.same_as(candidate))
            .or_else(|| kept.iter().position(|known| known.same_over_bluetooth(candidate)));
        match same {
            Some(index) => {
                let device = &mut devices[index];
                if device.battery.is_none() && candidate.device.battery.is_some() {
                    device.battery = candidate.device.battery;
                    device.charging = candidate.device.charging;
                }
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

fn name(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.chars().filter(|c| !c.is_control()).take(MAX_NAME_CHARS).collect())
}
