use super::descriptor::Reports;
use super::protocol::{self, Answer, Call, Length};
use crate::hidraw::channel::{Channel, Received};
use crate::name;
use nova_peripherals_api::{Battery, BatteryLevel, PeripheralKind};
use std::time::{Duration, Instant};

pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);
const ROOT: u8 = 0;
const ROOT_GET_FEATURE: u8 = 0;
const ROOT_PING: u8 = 1;
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
const UNIFIED_HAS_PERCENT: u8 = 0x02;
const LEVEL_FULL: u8 = 0x08;
const LEVEL_GOOD: u8 = 0x04;
const LEVEL_LOW: u8 = 0x02;
const LEVEL_CRITICAL: u8 = 0x01;
const UNIFIED_CHARGING: [u8; 2] = [1, 2];
const STATUS_CHARGING: [u8; 3] = [1, 2, 4];
const NOTIFICATIONS_REGISTER: u8 = 0x00;
const WIRELESS_NOTICES: (usize, u8) = (1, 0x01);
const MAX_NAME_BYTES: usize = 64;
const TRANSPORTS: usize = 4;

#[derive(Debug)]
pub enum Failure {
    Io(std::io::Error),
    Stopped,
    NoAnswer,
    Error(u8),
}

impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Ping {
    There,
    Receiver,
    Gone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub name: Option<String>,
    pub kind: PeripheralKind,
    pub unit_id: Option<[u8; 4]>,
    pub product_ids: Vec<u16>,
    pub battery: Option<BatteryFeature>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryFeature {
    Unified { index: u8, has_percent: bool },
    Status { index: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Charge {
    pub battery: Option<Battery>,
    pub charging: bool,
}

pub struct Conversation {
    channel: Channel,
    reports: Reports,
    pub heard: Vec<Vec<u8>>,
}

impl Conversation {
    pub fn new(channel: Channel, reports: Reports) -> Self {
        Self { channel, reports, heard: Vec::new() }
    }

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

    fn call(&mut self, device: u8, feature: u8, function: u8, params: &[u8]) -> Result<Vec<u8>, Failure> {
        self.call_as(Length::Long, Call { device, feature, function, params })
    }

    fn call_as(&mut self, length: Length, call: Call) -> Result<Vec<u8>, Failure> {
        self.send(protocol::request(self.reports, length, call))
    }

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

    pub fn ping(&mut self, device: u8) -> Result<Ping, Failure> {
        match self
            .call_as(Length::Short, Call { device, feature: ROOT, function: ROOT_PING, params: &[0, 0, PING_MARK] })
        {
            Ok(_) => Ok(Ping::There),
            Err(Failure::Error(protocol::ERROR_INVALID_COMMAND)) if device == protocol::OWN_INDEX => Ok(Ping::Receiver),
            Err(Failure::Error(_)) => Ok(Ping::Gone),
            Err(e) => Err(e),
        }
    }

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

    fn feature(&mut self, device: u8, id: u16) -> Result<Option<u8>, Failure> {
        let reply = self.call(device, ROOT, ROOT_GET_FEATURE, &id.to_be_bytes())?;
        Ok(reply.first().copied().filter(|index| *index != 0))
    }

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

    pub fn charge(&mut self, device: u8, feature: BatteryFeature) -> Result<Charge, Failure> {
        Ok(match feature {
            BatteryFeature::Unified { index, has_percent } => {
                unified(&self.call(device, index, UNIFIED_STATUS, &[])?, has_percent)
            }
            BatteryFeature::Status { index } => status(&self.call(device, index, BATTERY_STATUS_GET, &[])?),
        })
    }
}

fn optional<T>(result: Result<T, Failure>) -> Result<Option<T>, Failure> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(Failure::Error(_) | Failure::NoAnswer) => Ok(None),
        Err(e) => Err(e),
    }
}

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

fn status(reply: &[u8]) -> Charge {
    let byte = |at: usize| reply.get(at).copied().unwrap_or(0);
    let battery = (1..=100).contains(&byte(0)).then(|| Battery::Percent(byte(0)));
    Charge { battery, charging: STATUS_CHARGING.contains(&byte(2)) }
}

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
        let reply = [2, 0x12, 0x34, 0x56, 0x78, 0x00, 0x0c, 0x40, 0x7f, 0xc0, 0x94, 0, 0, 0, 0];
        assert_eq!(ids(&reply), (Some([0x12, 0x34, 0x56, 0x78]), vec![0x407f, 0xc094]));
        assert_eq!(ids(&[]), (None, Vec::new()));
    }
}
