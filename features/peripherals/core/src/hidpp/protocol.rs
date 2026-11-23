use super::descriptor::Reports;

pub const SHORT: u8 = 0x10;
pub const LONG: u8 = 0x11;
pub const VERY_LONG: u8 = 0x12;
const SHORT_BYTES: usize = 7;
const LONG_BYTES: usize = 20;
const VERY_LONG_BYTES: usize = 64;
const HEADER_BYTES: usize = 4;
pub const OWN_INDEX: u8 = 0xFF;
pub const PAIRED_INDICES: std::ops::RangeInclusive<u8> = 1..=6;
pub const SOFTWARE_ID: u8 = 0x0B;
const ERROR_1: u8 = 0x8F;
const ERROR_2: u8 = 0xFF;
const NOTIFICATIONS_FROM: u8 = 0x40;
const REGISTER_REPLIES_FROM: u8 = 0x80;
pub const ERROR_INVALID_COMMAND: u8 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Length {
    Short,
    Long,
}

#[derive(Debug, Clone, Copy)]
pub struct Call<'a> {
    pub device: u8,
    pub feature: u8,
    pub function: u8,
    pub params: &'a [u8],
}

pub fn request(reports: Reports, length: Length, call: Call) -> Vec<u8> {
    let Call { device, feature, function, params } = call;
    let fits = params.len() <= SHORT_BYTES - HEADER_BYTES;
    let short = reports.short && fits && (length == Length::Short || !reports.long);
    let (id, bytes) = if short { (SHORT, SHORT_BYTES) } else { (LONG, LONG_BYTES) };
    let mut message = vec![0; bytes];
    message[..HEADER_BYTES].copy_from_slice(&[id, device, feature, (function << 4) | SOFTWARE_ID]);
    let count = params.len().min(bytes - HEADER_BYTES);
    message[HEADER_BYTES..HEADER_BYTES + count].copy_from_slice(&params[..count]);
    message
}

const READ_REGISTER: u8 = 0x81;
const WRITE_REGISTER: u8 = 0x80;

pub fn read_register(register: u8) -> Vec<u8> {
    vec![SHORT, OWN_INDEX, READ_REGISTER, register, 0, 0, 0]
}

pub fn write_register(register: u8, value: [u8; 3]) -> Vec<u8> {
    vec![SHORT, OWN_INDEX, WRITE_REGISTER, register, value[0], value[1], value[2]]
}

#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Reply(Vec<u8>),
    Error(u8),
}

pub fn answer(request: &[u8], report: &[u8]) -> Option<Answer> {
    if !well_formed(report) || report[1] != request[1] {
        return None;
    }
    let (feature, function) = (request[2], request[3]);
    if report[2] == feature && report[3] == function {
        return Some(Answer::Reply(report[HEADER_BYTES..].to_vec()));
    }
    let errored =
        (report[2] == ERROR_1 && report.len() >= SHORT_BYTES) || (report[2] == ERROR_2 && report.len() >= LONG_BYTES);
    (errored && report[3] == feature && report[4] == function).then(|| Answer::Error(report[5]))
}

pub fn unasked(report: &[u8]) -> Option<u8> {
    let kind = report.get(2).copied().unwrap_or(ERROR_1);
    let notification = (NOTIFICATIONS_FROM..REGISTER_REPLIES_FROM).contains(&kind);
    let event = kind < NOTIFICATIONS_FROM && report.get(3).is_some_and(|byte| byte & 0x0F == 0);
    (well_formed(report) && (notification || event)).then_some(report[1])
}

fn well_formed(report: &[u8]) -> bool {
    match report.first() {
        Some(&SHORT) => report.len() == SHORT_BYTES,
        Some(&LONG) => report.len() == LONG_BYTES,
        Some(&VERY_LONG) => report.len() == VERY_LONG_BYTES,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: Reports = Reports { short: true, long: true };

    #[test]
    fn requests_are_as_long_as_asked_when_the_node_takes_it() {
        assert_eq!(
            request(BOTH, Length::Short, Call { device: 1, feature: 0, function: 1, params: &[0, 0, 0x5a] }),
            [0x10, 1, 0, 0x1b, 0, 0, 0x5a]
        );
        let long = request(BOTH, Length::Long, Call { device: 2, feature: 4, function: 0, params: &[0x10, 0x04] });
        assert_eq!((long.len(), &long[..6]), (20, &[0x11, 2, 4, 0x0b, 0x10, 0x04][..]));
        let long_only = Reports { short: false, long: true };
        assert_eq!(
            request(long_only, Length::Short, Call { device: 1, feature: 0, function: 1, params: &[] }).len(),
            20
        );
        let short_only = Reports { short: true, long: false };
        assert_eq!(
            request(short_only, Length::Long, Call { device: 1, feature: 0, function: 1, params: &[] }).len(),
            7
        );
    }

    #[test]
    fn replies_and_errors_are_matched_to_their_request() {
        let ping = request(BOTH, Length::Short, Call { device: 1, feature: 0, function: 1, params: &[0, 0, 0x5a] });
        let mut reply = vec![0x11, 1, 0, 0x1b, 4, 2, 0x5a];
        reply.resize(20, 0);
        assert_eq!(answer(&ping, &reply).map(|answer| matches!(answer, Answer::Reply(_))), Some(true));
        assert_eq!(answer(&ping, &[0x10, 1, 0x8f, 0, 0x1b, 0x09, 0]), Some(Answer::Error(0x09)));
        assert_eq!(answer(&ping, &[0x10, 2, 0x8f, 0, 0x1b, 0x09, 0]), None);
        let mut error = vec![0x11, 1, 0xff, 0, 0x1b, 0x05];
        error.resize(20, 0);
        assert_eq!(answer(&ping, &error), Some(Answer::Error(0x05)));
        assert_eq!(answer(&ping, &[0x10, 1, 0, 0x1b]), None, "too short for its id");
    }

    #[test]
    fn reports_nobody_asked_for_are_told_apart() {
        assert_eq!(unasked(&[0x10, 1, 0x41, 0x0c, 0x40, 0x7f, 0x40]), Some(1));
        assert_eq!(unasked(&[0x10, 1, 0, 0x1b, 0, 0, 0]), None, "a reply to novad");
        assert_eq!(unasked(&[0x10, 1, 0x8f, 0, 0x10, 9, 0]), None, "an error");
    }
}
