use super::{Found, LOG_SOURCE, Report, Running, Source, name, sysfs};
use futures_core::Stream;
use nova_carrier::log::LogLevel;
use nova_peripherals_api::{Battery, BatteryLevel, Peripheral, PeripheralKind, PeripheralSource};
use nova_session_bus::{Bus, Stop};
use std::collections::HashMap;
use std::future::poll_fn;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use zbus::wire::{OwnedObjectPath, OwnedVariant};
use zbus::{Connection, MatchRule, MessageStream};

pub const UPOWER: &str = "org.freedesktop.UPower";
pub const UPOWER_PATH: &str = "/org/freedesktop/UPower";
pub const DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";
pub const DEVICES_PATH: &str = "/org/freedesktop/UPower/devices";
const DISPLAY_DEVICE: &str = "DisplayDevice";
const DBUS_NAME: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
const ID_PREFIX: &str = "upower/";

const STATE_CHARGING: u32 = 1;

pub struct UPower {
    pub bus: Bus,
    pub sys_root: PathBuf,
}

struct Following(Arc<Stop>);

impl Running for Following {
    fn stop(&self) {
        self.0.stop();
    }
}

impl Source for UPower {
    fn start(self: Box<Self>, report: Report) -> Box<dyn Running> {
        let stop = Arc::new(Stop::default());
        let following = Arc::clone(&stop);
        std::thread::spawn(move || {
            if let Err(e) = zbus::block_on(follow(&self, &report, &following)) {
                report.logger().info(LOG_SOURCE, &format!("no batteries from UPower: {e}"));
                report.found(Vec::new());
            }
        });
        Box::new(Following(stop))
    }
}

async fn subscribe(connection: &Connection) -> zbus::Result<(MessageStream, MessageStream, MessageStream)> {
    let signal = |interface: &'static str, member: &'static str| {
        MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(interface).member(member)
    };
    let owner_rule = signal(DBUS_NAME, "NameOwnerChanged").sender(DBUS_NAME).arg(0, UPOWER).build()?;
    let owners = MessageStream::for_match_rule(owner_rule, connection, None).await?;
    let devices_rule =
        MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(UPOWER).path(UPOWER_PATH).build()?;
    let comings = MessageStream::for_match_rule(devices_rule, connection, None).await?;
    let changed_rule = signal(PROPERTIES, "PropertiesChanged").path_namespace(DEVICES_PATH).build()?;
    let changes = MessageStream::for_match_rule(changed_rule, connection, None).await?;
    Ok((owners, comings, changes))
}

async fn follow(upower: &UPower, report: &Report, stop: &Stop) -> zbus::Result<()> {
    let connection = upower.bus.builder().method_timeout(CALL_TIMEOUT).build().await?;
    let (mut owners, mut comings, mut changes) = subscribe(&connection).await?;

    start(&connection, report).await;
    let mut owner = name_owner(&connection).await;
    let mut devices = match &owner {
        Some(_) => list(&connection, &upower.sys_root, report, stop).await,
        None => {
            report.logger().info(LOG_SOURCE, "UPower isn't running; following it in case it starts");
            HashMap::new()
        }
    };
    report.found(devices.values().flatten().cloned().collect());

    loop {
        let next = poll_fn(|cx| {
            if stop.poll(cx).is_ready() {
                return Poll::Ready(None);
            }
            for (kind, stream) in
                [(Signal::Owner, &mut owners), (Signal::Device, &mut comings), (Signal::Changed, &mut changes)]
            {
                if let Poll::Ready(message) = Pin::new(stream).poll_next(cx) {
                    return Poll::Ready(message.map(|message| (kind, message)));
                }
            }
            Poll::Pending
        })
        .await;
        let Some((kind, message)) = next else { return Ok(()) };
        let message = message?;
        let sender = message.header().sender().map(|sender| sender.to_string());
        let from_upower = sender.is_some() && sender == owner;
        match kind {
            Signal::Owner => {
                let Ok((_name, _old, new)) = message.body().deserialize::<(String, String, String)>() else { continue };
                if new.is_empty() {
                    report.logger().info(LOG_SOURCE, "UPower stopped");
                    owner = None;
                    devices.clear();
                } else {
                    report.logger().info(LOG_SOURCE, "UPower started");
                    owner = Some(new);
                    devices = list(&connection, &upower.sys_root, report, stop).await;
                }
            }
            Signal::Device if from_upower => {
                let member = message.header().member().map(|member| member.to_string());
                let Ok(path) = message.body().deserialize::<OwnedObjectPath>() else { continue };
                match member.as_deref() {
                    Some("DeviceAdded") => {
                        let found = read(&connection, &path, &upower.sys_root, report).await;
                        devices.insert(path, found);
                    }
                    Some("DeviceRemoved") => {
                        devices.remove(&path);
                    }
                    _ => continue,
                }
            }
            Signal::Changed if from_upower => {
                let Some(path) = message.header().path().map(|path| OwnedObjectPath::from(path.to_owned())) else {
                    continue;
                };
                let Ok((interface, ..)) =
                    message.body().deserialize::<(String, HashMap<String, OwnedVariant>, Vec<String>)>()
                else {
                    continue;
                };
                if interface != DEVICE_INTERFACE || !devices.contains_key(&path) {
                    continue;
                }
                let found = read(&connection, &path, &upower.sys_root, report).await;
                devices.insert(path, found);
            }
            Signal::Device | Signal::Changed => continue,
        }
        report.found(devices.values().flatten().cloned().collect());
    }
}

#[derive(Clone, Copy)]
enum Signal {
    Owner,
    Device,
    Changed,
}

async fn start(connection: &Connection, report: &Report) {
    if let Err(e) =
        connection.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "StartServiceByName", &(UPOWER, 0u32)).await
    {
        report.logger().info(LOG_SOURCE, &format!("couldn't start UPower: {e}"));
    }
}

async fn name_owner(connection: &Connection) -> Option<String> {
    let reply =
        connection.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "GetNameOwner", &(UPOWER,)).await.ok()?;
    reply.body().deserialize().ok()
}

async fn list(
    connection: &Connection,
    sys_root: &Path,
    report: &Report,
    stop: &Stop,
) -> HashMap<OwnedObjectPath, Option<Found>> {
    let paths: Vec<OwnedObjectPath> = match connection
        .call_method(Some(UPOWER), UPOWER_PATH, Some(UPOWER), "EnumerateDevices", &())
        .await
        .and_then(|reply| reply.body().deserialize())
    {
        Ok(paths) => paths,
        Err(e) => {
            report.logger().warn(LOG_SOURCE, &format!("can't list UPower's devices: {e}"));
            return HashMap::new();
        }
    };
    let mut devices = HashMap::new();
    for path in paths {
        if stop.stopped() {
            break;
        }
        let found = read(connection, &path, sys_root, report).await;
        devices.insert(path, found);
    }
    devices
}

async fn read(connection: &Connection, path: &OwnedObjectPath, sys_root: &Path, report: &Report) -> Option<Found> {
    let reply = connection
        .call_method(Some(UPOWER), path.as_str(), Some(PROPERTIES), "GetAll", &(DEVICE_INTERFACE,))
        .await
        .and_then(|reply| reply.body().deserialize::<HashMap<String, OwnedVariant>>());
    match reply {
        Ok(properties) => found(path.as_str(), &Properties::from_variants(&properties), sys_root),
        Err(e) => {
            report.logger().log(LogLevel::Debug, LOG_SOURCE, &format!("can't read {path}: {e}"));
            None
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Properties {
    pub native_path: String,
    pub vendor: String,
    pub model: String,
    pub serial: String,
    pub kind: u32,
    pub power_supply: bool,
    pub percentage: f64,
    pub battery_level: u32,
    pub state: u32,
    pub is_present: bool,
}

impl Properties {
    fn from_variants(properties: &HashMap<String, OwnedVariant>) -> Self {
        let text = |name: &str| {
            properties.get(name).and_then(|value| String::try_from(value.try_clone().ok()?).ok()).unwrap_or_default()
        };
        let number = |name: &str| properties.get(name).and_then(|value| u32::try_from(value).ok()).unwrap_or(0);
        let flag = |name: &str, default: bool| {
            properties.get(name).and_then(|value| bool::try_from(value).ok()).unwrap_or(default)
        };
        Self {
            native_path: text("NativePath"),
            vendor: text("Vendor"),
            model: text("Model"),
            serial: text("Serial"),
            kind: number("Type"),
            power_supply: flag("PowerSupply", false),
            percentage: properties.get("Percentage").and_then(|value| f64::try_from(value).ok()).unwrap_or(0.0),
            battery_level: number("BatteryLevel"),
            state: number("State"),
            is_present: flag("IsPresent", true),
        }
    }
}

pub fn found(path: &str, properties: &Properties, sys_root: &Path) -> Option<Found> {
    let object = path.rsplit('/').next().filter(|object| !object.is_empty() && *object != DISPLAY_DEVICE)?;
    if properties.power_supply || !properties.is_present {
        return None;
    }
    let kind = kind(properties.kind)?;
    let location = sysfs::locate(sys_root, &properties.native_path);
    let device = Peripheral {
        id: format!("{ID_PREFIX}{object}"),
        name: name(&properties.model).or_else(|| name(&properties.vendor)).unwrap_or_else(|| object.to_string()),
        kind,
        link: location.link,
        battery: battery(properties),
        charging: properties.state == STATE_CHARGING,
        source: PeripheralSource::Upower,
    };
    Some(Found { device, usb_ids: location.usb_ids, serial: name(&properties.serial) })
}

fn kind(kind: u32) -> Option<PeripheralKind> {
    Some(match kind {
        0 | 2 => PeripheralKind::Other,
        5 => PeripheralKind::Mouse,
        6 => PeripheralKind::Keyboard,
        10 => PeripheralKind::Tablet,
        12 => PeripheralKind::GamingInput,
        13 => PeripheralKind::Pen,
        14 => PeripheralKind::Touchpad,
        17 => PeripheralKind::Headset,
        18 => PeripheralKind::Speakers,
        19 => PeripheralKind::Headphones,
        22 => PeripheralKind::RemoteControl,
        26 => PeripheralKind::Wearable,
        21 | 27 | 28 => PeripheralKind::Other,
        _ => return None,
    })
}

fn battery(properties: &Properties) -> Option<Battery> {
    let level = match properties.battery_level {
        3 => Some(BatteryLevel::Low),
        4 => Some(BatteryLevel::Critical),
        6 => Some(BatteryLevel::Normal),
        7 => Some(BatteryLevel::High),
        8 => Some(BatteryLevel::Full),
        _ => None,
    };
    if let Some(level) = level {
        return Some(Battery::Level(level));
    }
    let unknown = properties.battery_level == 0 && properties.percentage <= 0.0;
    (!unknown && properties.percentage.is_finite())
        .then(|| Battery::Percent(properties.percentage.clamp(0.0, 100.0).round() as u8))
}
