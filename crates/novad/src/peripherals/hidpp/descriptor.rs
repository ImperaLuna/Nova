//! Which HID++ reports a hidraw node takes, from its HID report
//! descriptor: HID++ is Logitech's vendor protocol on usage page `0xFF00`
//! (`0xFF43` on newer Bluetooth devices),
//! in a short report (id `0x10`, 7 bytes) and a long one (`0x11`, 20
//! bytes). A receiver has it on one of its interfaces; the others (the
//! mouse's and keyboard's input) don't.

use super::protocol::{LONG, SHORT};
use crate::peripherals::hidraw::descriptor::{REPORT_ID, USAGE_PAGE, items};

/// The vendor usage pages HID++ reports are on.
const HIDPP_USAGE_PAGES: [u32; 2] = [0xFF00, 0xFF43];

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
    for item in items(descriptor) {
        match item.tag {
            USAGE_PAGE => page = item.value,
            REPORT_ID if HIDPP_USAGE_PAGES.contains(&page) => match item.value {
                id if id == u32::from(SHORT) => found.short = true,
                id if id == u32::from(LONG) => found.long = true,
                _ => {}
            },
            _ => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peripherals::fake_hidraw::hex;

    #[test]
    fn hidpp_reports_are_found_on_the_vendor_page_only() {
        // Usage Page 0xFF00, Report ID 0x10, ..., Report ID 0x11.
        assert_eq!(reports(&hex("0600ff0901a1018510750895061500")), Reports { short: true, long: false });
        assert_eq!(reports(&hex("0600ff8510851185")), Reports { short: true, long: true }, "a cut-off item ends it");
        // A mouse's Report ID 0x10 on the Generic Desktop page isn't HID++.
        assert_eq!(reports(&hex("05010902a1018510")), Reports::default());
        assert_eq!(reports(&hex("0643ff0a02028511")), Reports { short: false, long: true }, "Bluetooth's page");
        assert_eq!(reports(&[]), Reports::default());
    }
}
