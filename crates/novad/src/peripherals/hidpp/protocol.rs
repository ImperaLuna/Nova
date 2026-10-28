//! HID++ messages: building requests and knowing their replies.
//!
//! A message is a report: its id (short `0x10`, long `0x11`, very long
//! `0x12`), the device index (`0xFF` for a receiver itself or a device on
//! its own cable, 1 to 6 for a receiver's paired devices), then a feature
//! index and a byte holding the function (high nibble) and the sender's
//! software id (low nibble), then parameters. A reply repeats the first
//! four bytes. Errors come as HID++ 1.0's (`0x8F`, the request's feature
//! index and function, then the code; a receiver says so for a paired
//! device that's off) or HID++ 2.0's (`0xFF`, likewise). Anything else is
//! a report nobody asked for: a device connecting, a battery changing.

use super::descriptor::Reports;

pub const SHORT: u8 = 0x10;
pub const LONG: u8 = 0x11;
pub const VERY_LONG: u8 = 0x12;
const SHORT_BYTES: usize = 7;
const LONG_BYTES: usize = 20;
const VERY_LONG_BYTES: usize = 64;
/// A request's bytes before its parameters.
const HEADER_BYTES: usize = 4;
/// A receiver itself, or a device that's not behind one.
pub const OWN_INDEX: u8 = 0xFF;
/// The device indices a receiver's paired devices have.
pub const PAIRED_INDICES: std::ops::RangeInclusive<u8> = 1..=6;
/// Who novad is, in a request's low nibble, so its replies are told apart
/// from the kernel's (1) and other programs' (Solaar's).
pub const SOFTWARE_ID: u8 = 0x0B;
/// What a HID++ 1.0 and a HID++ 2.0 error have where a feature index goes.
const ERROR_1: u8 = 0x8F;
const ERROR_2: u8 = 0xFF;
/// HID++ 1.0's notifications' sub ids start here.
const NOTIFICATIONS_FROM: u8 = 0x40;
/// ...and its register replies (and errors) here.
const REGISTER_REPLIES_FROM: u8 = 0x80;
/// HID++ 1.0's "no such command": a receiver's answer to HID++ 2.0's ping.
pub const ERROR_INVALID_COMMAND: u8 = 0x01;

/// How long a request is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Length {
    /// Short when the node takes short reports: a ping, which a HID++ 1.0
    /// receiver must understand too.
    Short,
    /// Long when the node takes long reports: HID++ 2.0 replies are.
    Long,
}

/// A request to `device`: function `function` of the feature at
/// `feature`, with `params`, `length` long if `reports` allows.
pub fn request(reports: Reports, length: Length, device: u8, feature: u8, function: u8, params: &[u8]) -> Vec<u8> {
    let fits = params.len() <= SHORT_BYTES - HEADER_BYTES;
    let short = reports.short && fits && (length == Length::Short || !reports.long);
    let (id, bytes) = if short { (SHORT, SHORT_BYTES) } else { (LONG, LONG_BYTES) };
    let mut message = vec![0; bytes];
    message[..HEADER_BYTES].copy_from_slice(&[id, device, feature, (function << 4) | SOFTWARE_ID]);
    let count = params.len().min(bytes - HEADER_BYTES);
    message[HEADER_BYTES..HEADER_BYTES + count].copy_from_slice(&params[..count]);
    message
}

/// HID++ 1.0's register requests to a receiver: short, read or write, then
/// the register's address and up to 3 bytes.
const READ_REGISTER: u8 = 0x81;
const WRITE_REGISTER: u8 = 0x80;

/// A request reading register `register` of the receiver.
pub fn read_register(register: u8) -> Vec<u8> {
    vec![SHORT, OWN_INDEX, READ_REGISTER, register, 0, 0, 0]
}

/// A request writing `value` to register `register` of the receiver.
pub fn write_register(register: u8, value: [u8; 3]) -> Vec<u8> {
    vec![SHORT, OWN_INDEX, WRITE_REGISTER, register, value[0], value[1], value[2]]
}

/// What a report is to a request.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    /// Its reply, with these parameters.
    Reply(Vec<u8>),
    /// It failed with this error code.
    Error(u8),
}

/// `report` as the answer to `request`, if it is one.
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

/// The device index of a report nobody asked for: a HID++ 1.0
/// notification (sub ids `0x40` to `0x7F`, a device connecting or going)
/// or a HID++ 2.0 event (a feature index, software id 0, a battery
/// changing). Replies, errors and register answers (`0x80` and up) aren't.
pub fn unasked(report: &[u8]) -> Option<u8> {
    let kind = report.get(2).copied().unwrap_or(ERROR_1);
    let notification = (NOTIFICATIONS_FROM..REGISTER_REPLIES_FROM).contains(&kind);
    let event = kind < NOTIFICATIONS_FROM && report.get(3).is_some_and(|byte| byte & 0x0F == 0);
    (well_formed(report) && (notification || event)).then_some(report[1])
}

/// A HID++ report of the length its id says.
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
        assert_eq!(request(BOTH, Length::Short, 1, 0, 1, &[0, 0, 0x5a]), [0x10, 1, 0, 0x1b, 0, 0, 0x5a]);
        let long = request(BOTH, Length::Long, 2, 4, 0, &[0x10, 0x04]);
        assert_eq!((long.len(), &long[..6]), (20, &[0x11, 2, 4, 0x0b, 0x10, 0x04][..]));
        let long_only = Reports { short: false, long: true };
        assert_eq!(request(long_only, Length::Short, 1, 0, 1, &[]).len(), 20);
        let short_only = Reports { short: true, long: false };
        assert_eq!(request(short_only, Length::Long, 1, 0, 1, &[]).len(), 7);
    }

    #[test]
    fn replies_and_errors_are_matched_to_their_request() {
        let ping = request(BOTH, Length::Short, 1, 0, 1, &[0, 0, 0x5a]);
        let mut reply = vec![0x11, 1, 0, 0x1b, 4, 2, 0x5a];
        reply.resize(20, 0);
        assert_eq!(answer(&ping, &reply).map(|answer| matches!(answer, Answer::Reply(_))), Some(true));
        // The receiver for a device that's off; for another device; a
        // device's own error.
        assert_eq!(answer(&ping, &[0x10, 1, 0x8f, 0, 0x1b, 0x09, 0]), Some(Answer::Error(0x09)));
        assert_eq!(answer(&ping, &[0x10, 2, 0x8f, 0, 0x1b, 0x09, 0]), None);
        let mut error = vec![0x11, 1, 0xff, 0, 0x1b, 0x05];
        error.resize(20, 0);
        assert_eq!(answer(&ping, &error), Some(Answer::Error(0x05)));
        assert_eq!(answer(&ping, &[0x10, 1, 0, 0x1b]), None, "too short for its id");
    }

    #[test]
    fn reports_nobody_asked_for_are_told_apart() {
        // A receiver's "device connected" (HID++ 1.0 sub id 0x41).
        assert_eq!(unasked(&[0x10, 1, 0x41, 0x0c, 0x40, 0x7f, 0x40]), Some(1));
        assert_eq!(unasked(&[0x10, 1, 0, 0x1b, 0, 0, 0]), None, "a reply to novad");
        assert_eq!(unasked(&[0x10, 1, 0x8f, 0, 0x10, 9, 0]), None, "an error");
    }
}
