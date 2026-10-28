//! Talking HID++ 2.0 to one device on a hidraw node: is it there, what is
//! it, and how charged is it.
//!
//! Every request waits for its own answer ([`ANSWER_TIMEOUT`] at most);
//! reports nobody asked for that come meanwhile are kept in
//! [`Conversation::heard`] for the reader to act on. A feature is used by
//! its index on that device, which the root feature (always index 0) gives
//! for its id. The battery comes from UNIFIED_BATTERY (`0x1004`, newer
//! devices) or else BATTERY_STATUS (`0x1000`).

use super::channel::{Channel, Received};
use super::descriptor::Reports;
use super::protocol::{self, Answer, Length};
use crate::peripherals::name;
use nova_protocol::{Battery, BatteryLevel, PeripheralKind};
use std::time::{Duration, Instant};

/// How long a device or its receiver may take to answer: a paired device
/// answers over the air.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);
/// The root feature's index, and its functions.
const ROOT: u8 = 0;
const ROOT_GET_FEATURE: u8 = 0;
const ROOT_PING: u8 = 1;
/// Sent with a ping and echoed back.
const PING_MARK: u8 = 0x5A;
const DEVICE_INFO: u16 = 0x0003;
const DEVICE_INFO_GET: u8 = 0;
const DEVICE_NAME: u16 = 0x0005;
const NAME_LENGTH: u8 = 0;
const NAME_PART: u8 = 1;
const NAME_TYPE: u8 = 2;
const BATTERY_STATUS: u16 = 0x1000;
const BATTERY_STATUS_GET: u8 = 0;
const UNIFIED_BATTERY: u16 = 0x1004;
const UNIFIED_CAPABILITIES: u8 = 0;
const UNIFIED_STATUS: u8 = 1;
/// UNIFIED_BATTERY's capability flag for a percentage, not only levels.
const UNIFIED_HAS_PERCENT: u8 = 0x02;
/// UNIFIED_BATTERY's level bits.
const LEVEL_FULL: u8 = 0x08;
const LEVEL_GOOD: u8 = 0x04;
const LEVEL_LOW: u8 = 0x02;
const LEVEL_CRITICAL: u8 = 0x01;
/// UNIFIED_BATTERY's charging states: charging, slowly.
const UNIFIED_CHARGING: [u8; 2] = [1, 2];
/// BATTERY_STATUS's: recharging, almost full, slow recharge.
const STATUS_CHARGING: [u8; 3] = [1, 2, 4];
/// A receiver's HID++ 1.0 register of what it reports by itself, and its
/// wireless bit (`0x000100`: which byte, which bit).
const NOTIFICATIONS_REGISTER: u8 = 0x00;
const WIRELESS_NOTICES: (usize, u8) = (1, 0x01);
/// The longest name read: the sidebar shows less.
const MAX_NAME_BYTES: usize = 64;
/// DEVICE_INFO's transports, in the order their product ids follow:
/// Bluetooth, Bluetooth LE, a receiver's (eQuad), USB.
const TRANSPORTS: usize = 4;

/// Why a request got no reply.
#[derive(Debug)]
pub enum Failure {
    Io(std::io::Error),
    Stopped,
    /// Nothing came in [`ANSWER_TIMEOUT`].
    NoAnswer,
    /// It answered with this HID++ error code.
    Error(u8),
}

impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Whether a device index answered HID++ 2.0's ping.
#[derive(Debug, PartialEq, Eq)]
pub enum Ping {
    /// It's there and speaks HID++ 2.0 or later.
    There,
    /// A receiver, speaking HID++ 1.0 only.
    Receiver,
    /// Nothing there (or it's off).
    Gone,
}

/// What a device is, read once while it stays connected.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub name: Option<String>,
    pub kind: PeripheralKind,
    /// Its unit id, which tells it from another of its model.
    pub unit_id: Option<[u8; 4]>,
    /// Its model's product ids, on each transport it has.
    pub product_ids: Vec<u16>,
    pub battery: Option<BatteryFeature>,
}

/// Where a device's battery is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryFeature {
    Unified { index: u8, has_percent: bool },
    Status { index: u8 },
}

/// A battery reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Charge {
    pub battery: Option<Battery>,
    pub charging: bool,
}

pub struct Conversation {
    channel: Channel,
    reports: Reports,
    /// Reports nobody asked for that came while waiting for an answer.
    pub heard: Vec<Vec<u8>>,
}

impl Conversation {
    pub fn new(channel: Channel, reports: Reports) -> Self {
        Self { channel, reports, heard: Vec::new() }
    }

    /// The next report nobody asked for, waiting until `until` at most.
    pub fn wait(&mut self, until: Instant) -> Result<Option<Vec<u8>>, Failure> {
        loop {
            match self.channel.receive(until)? {
                Received::Report(report) if protocol::unasked(&report).is_some() => return Ok(Some(report)),
                Received::Report(_) => {}
                Received::TimedOut => return Ok(None),
                Received::Stopped => return Err(Failure::Stopped),
            }
        }
    }

    /// Function `function` of the feature at `feature` on `device`.
    fn call(&mut self, device: u8, feature: u8, function: u8, params: &[u8]) -> Result<Vec<u8>, Failure> {
        self.call_as(Length::Long, device, feature, function, params)
    }

    fn call_as(
        &mut self,
        length: Length,
        device: u8,
        feature: u8,
        function: u8,
        params: &[u8],
    ) -> Result<Vec<u8>, Failure> {
        self.send(protocol::request(self.reports, length, device, feature, function, params))
    }

    /// Sends `request` and waits for its answer.
    fn send(&mut self, request: Vec<u8>) -> Result<Vec<u8>, Failure> {
        if !self.channel.send(&request)? {
            return Err(Failure::Stopped);
        }
        let until = Instant::now() + ANSWER_TIMEOUT;
        loop {
            match self.channel.receive(until)? {
                Received::Report(report) => match protocol::answer(&request, &report) {
                    Some(Answer::Reply(params)) => return Ok(params),
                    Some(Answer::Error(code)) => return Err(Failure::Error(code)),
                    None if protocol::unasked(&report).is_some() => self.heard.push(report),
                    None => {}
                },
                Received::TimedOut => return Err(Failure::NoAnswer),
                Received::Stopped => return Err(Failure::Stopped),
            }
        }
    }

    /// Whether `device` is there.
    pub fn ping(&mut self, device: u8) -> Result<Ping, Failure> {
        match self.call_as(Length::Short, device, ROOT, ROOT_PING, &[0, 0, PING_MARK]) {
            Ok(_) => Ok(Ping::There),
            Err(Failure::Error(protocol::ERROR_INVALID_COMMAND)) if device == protocol::OWN_INDEX => Ok(Ping::Receiver),
            Err(Failure::Error(_) | Failure::NoAnswer) => Ok(Ping::Gone),
            Err(e) => Err(e),
        }
    }

    /// Has a receiver say when a paired device connects or goes (HID++
    /// 1.0 notice `0x41`), as Solaar and the kernel's driver do: the
    /// wireless bit of its notifications register, keeping the bits others
    /// set. A device waking from sleep is then read at once, not on the
    /// next poll.
    pub fn follow_connections(&mut self) -> Result<(), Failure> {
        if !self.reports.short {
            return Ok(());
        }
        let flags = self.send(protocol::read_register(NOTIFICATIONS_REGISTER))?;
        let mut value = [0; 3];
        for (byte, flag) in value.iter_mut().zip(&flags) {
            *byte = *flag;
        }
        if value[WIRELESS_NOTICES.0] & WIRELESS_NOTICES.1 != 0 {
            return Ok(());
        }
        value[WIRELESS_NOTICES.0] |= WIRELESS_NOTICES.1;
        self.send(protocol::write_register(NOTIFICATIONS_REGISTER, value)).map(|_| ())
    }

    /// The index of feature `id` on `device`, if it has it.
    fn feature(&mut self, device: u8, id: u16) -> Result<Option<u8>, Failure> {
        let reply = self.call(device, ROOT, ROOT_GET_FEATURE, &id.to_be_bytes())?;
        Ok(reply.first().copied().filter(|index| *index != 0))
    }

    /// What `device` is. Its name, type and ids are nice to have: one it
    /// won't give is left out, and only its battery feature must be found.
    pub fn info(&mut self, device: u8) -> Result<Info, Failure> {
        let mut info =
            Info { name: None, kind: PeripheralKind::Other, unit_id: None, product_ids: Vec::new(), battery: None };
        if let Some(index) = optional(self.feature(device, DEVICE_NAME))?.flatten() {
            info.name = optional(self.name(device, index))?.flatten();
            let kind = optional(self.call(device, index, NAME_TYPE, &[]))?;
            info.kind = kind.as_deref().and_then(<[u8]>::first).map_or(PeripheralKind::Other, |kind| kind_of(*kind));
        }
        if let Some(index) = optional(self.feature(device, DEVICE_INFO))?.flatten()
            && let Some(reply) = optional(self.call(device, index, DEVICE_INFO_GET, &[]))?
        {
            (info.unit_id, info.product_ids) = ids(&reply);
        }
        info.battery = if let Some(index) = self.feature(device, UNIFIED_BATTERY)? {
            let capabilities = self.call(device, index, UNIFIED_CAPABILITIES, &[])?;
            let has_percent = capabilities.get(1).is_some_and(|flags| flags & UNIFIED_HAS_PERCENT != 0);
            Some(BatteryFeature::Unified { index, has_percent })
        } else {
            self.feature(device, BATTERY_STATUS)?.map(|index| BatteryFeature::Status { index })
        };
        Ok(info)
    }

    /// Its name, read in parts.
    fn name(&mut self, device: u8, index: u8) -> Result<Option<String>, Failure> {
        let length = self.call(device, index, NAME_LENGTH, &[])?;
        let length = length.first().map_or(0, |length| usize::from(*length)).min(MAX_NAME_BYTES);
        let mut bytes = Vec::new();
        while bytes.len() < length {
            let offset = u8::try_from(bytes.len()).unwrap_or(u8::MAX);
            let part = self.call(device, index, NAME_PART, &[offset])?;
            if part.is_empty() {
                break;
            }
            bytes.extend(part);
        }
        bytes.truncate(length);
        Ok(name(&String::from_utf8_lossy(&bytes)))
    }

    /// How charged it is now.
    pub fn charge(&mut self, device: u8, feature: BatteryFeature) -> Result<Charge, Failure> {
        Ok(match feature {
            BatteryFeature::Unified { index, has_percent } => {
                unified(&self.call(device, index, UNIFIED_STATUS, &[])?, has_percent)
            }
            BatteryFeature::Status { index } => status(&self.call(device, index, BATTERY_STATUS_GET, &[])?),
        })
    }
}

/// `result`, or `None` when the device said no or didn't answer; a stop or
/// a lost node still ends it.
fn optional<T>(result: Result<T, Failure>) -> Result<Option<T>, Failure> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(Failure::Error(_) | Failure::NoAnswer) => Ok(None),
        Err(e) => Err(e),
    }
}

/// UNIFIED_BATTERY's status: percentage, level bits, charging state.
fn unified(reply: &[u8], has_percent: bool) -> Charge {
    let byte = |at: usize| reply.get(at).copied().unwrap_or(0);
    let level = byte(1);
    let battery = if has_percent && byte(0) <= 100 {
        Some(Battery::Percent(byte(0)))
    } else if level & LEVEL_FULL != 0 {
        Some(Battery::Level(BatteryLevel::Full))
    } else if level & LEVEL_GOOD != 0 {
        Some(Battery::Level(BatteryLevel::Normal))
    } else if level & LEVEL_LOW != 0 {
        Some(Battery::Level(BatteryLevel::Low))
    } else if level & LEVEL_CRITICAL != 0 {
        Some(Battery::Level(BatteryLevel::Critical))
    } else {
        None
    };
    Charge { battery, charging: UNIFIED_CHARGING.contains(&byte(2)) }
}

/// BATTERY_STATUS's: percentage (0 while unknown), next level, state.
fn status(reply: &[u8]) -> Charge {
    let byte = |at: usize| reply.get(at).copied().unwrap_or(0);
    let battery = (1..=100).contains(&byte(0)).then(|| Battery::Percent(byte(0)));
    Charge { battery, charging: STATUS_CHARGING.contains(&byte(2)) }
}

/// DEVICE_INFO's unit id and its model's product ids: entity count, unit
/// id (4 bytes), transports (2, a bit each), model id (a product id per
/// transport, 2 bytes each, in [`TRANSPORTS`] order).
fn ids(reply: &[u8]) -> (Option<[u8; 4]>, Vec<u16>) {
    let unit_id = reply.get(1..5).and_then(|bytes| <[u8; 4]>::try_from(bytes).ok()).filter(|id| *id != [0; 4]);
    let transports = reply.get(5..7).map_or(0, |bytes| u16::from_be_bytes([bytes[0], bytes[1]]));
    let mut product_ids = Vec::new();
    let mut at = 7;
    for transport in 0..TRANSPORTS {
        if transports & (1 << transport) == 0 {
            continue;
        }
        if let Some(bytes) = reply.get(at..at + 2) {
            let id = u16::from_be_bytes([bytes[0], bytes[1]]);
            if id != 0 && !product_ids.contains(&id) {
                product_ids.push(id);
            }
        }
        at += 2;
    }
    (unit_id, product_ids)
}

/// DEVICE_NAME's device types, for the icon.
fn kind_of(kind: u8) -> PeripheralKind {
    match kind {
        0 | 2 => PeripheralKind::Keyboard,
        1 | 6 => PeripheralKind::RemoteControl,
        3 | 5 => PeripheralKind::Mouse,
        4 => PeripheralKind::Touchpad,
        8 => PeripheralKind::Headset,
        10..=12 => PeripheralKind::GamingInput,
        14 => PeripheralKind::Speakers,
        _ => PeripheralKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_battery_gives_a_percent_or_a_level() {
        assert_eq!(
            unified(&[55, LEVEL_GOOD, 0, 0], true),
            Charge { battery: Some(Battery::Percent(55)), charging: false }
        );
        assert_eq!(
            unified(&[0, LEVEL_LOW | LEVEL_CRITICAL, 1, 1], false),
            Charge { battery: Some(Battery::Level(BatteryLevel::Low)), charging: true }
        );
        assert_eq!(unified(&[], false).battery, None);
    }

    #[test]
    fn battery_status_gives_a_percent_once_known() {
        assert_eq!(status(&[40, 20, 1]), Charge { battery: Some(Battery::Percent(40)), charging: true });
        assert_eq!(status(&[0, 0, 0]).battery, None);
    }

    #[test]
    fn device_info_gives_the_unit_and_its_product_ids() {
        // Unit 12345678; transports eQuad + USB; model ids 407f, c094.
        let reply = [2, 0x12, 0x34, 0x56, 0x78, 0x00, 0x0c, 0x40, 0x7f, 0xc0, 0x94, 0, 0, 0, 0];
        assert_eq!(ids(&reply), (Some([0x12, 0x34, 0x56, 0x78]), vec![0x407f, 0xc094]));
        assert_eq!(ids(&[]), (None, Vec::new()));
    }
}
