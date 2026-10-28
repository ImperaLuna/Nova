//! Which HID++ reports a hidraw node takes, from its HID report
//! descriptor: HID++ is Logitech's vendor protocol on usage page `0xFF00`,
//! in a short report (id `0x10`, 7 bytes) and a long one (`0x11`, 20
//! bytes). A receiver has it on one of its interfaces; the others (the
//! mouse's and keyboard's input) don't.
//!
//! The descriptor is a list of items, each a prefix byte (tag, type and
//! data size) and up to 4 bytes of data; only Usage Page and Report ID
//! matter here.

use super::protocol::{LONG, SHORT};

/// The vendor usage page HID++ reports are on.
const HIDPP_USAGE_PAGE: u32 = 0xFF00;
/// Global items: Usage Page, and Report ID (tag and type, size bits off).
const USAGE_PAGE_ITEM: u8 = 0x04;
const REPORT_ID_ITEM: u8 = 0x84;
/// A long item's prefix, followed by its data size and tag.
const LONG_ITEM: u8 = 0xFE;
/// The data sizes a short item's two low bits stand for.
const DATA_SIZES: [usize; 4] = [0, 1, 2, 4];

/// The HID++ reports a node takes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reports {
    pub short: bool,
    pub long: bool,
}

impl Reports {
    /// Whether it speaks HID++ at all.
    pub fn any(self) -> bool {
        self.short || self.long
    }
}

/// The HID++ reports in report descriptor `descriptor`.
pub fn reports(descriptor: &[u8]) -> Reports {
    let mut found = Reports::default();
    let mut page = 0;
    let mut at = 0;
    while at < descriptor.len() {
        let prefix = descriptor[at];
        if prefix == LONG_ITEM {
            let size = descriptor.get(at + 1).copied().map_or(0, usize::from);
            at += 3 + size;
            continue;
        }
        let size = DATA_SIZES[usize::from(prefix & 0x03)];
        let Some(data) = descriptor.get(at + 1..at + 1 + size) else { break };
        let value = data.iter().rev().fold(0u32, |value, byte| (value << 8) | u32::from(*byte));
        match prefix & 0xFC {
            USAGE_PAGE_ITEM => page = value,
            REPORT_ID_ITEM if page == HIDPP_USAGE_PAGE => match value {
                id if id == u32::from(SHORT) => found.short = true,
                id if id == u32::from(LONG) => found.long = true,
                _ => {}
            },
            _ => {}
        }
        at += 1 + size;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap()).collect()
    }

    #[test]
    fn hidpp_reports_are_found_on_the_vendor_page_only() {
        // Usage Page 0xFF00, Report ID 0x10, ..., Report ID 0x11.
        assert_eq!(reports(&hex("0600ff0901a1018510750895061500")), Reports { short: true, long: false });
        assert_eq!(reports(&hex("0600ff8510851185")), Reports { short: true, long: true }, "a cut-off item ends it");
        // A mouse's Report ID 0x10 on the Generic Desktop page isn't HID++.
        assert_eq!(reports(&hex("05010902a1018510")), Reports::default());
        assert_eq!(reports(&[]), Reports::default());
    }
}
