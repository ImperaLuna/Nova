use super::protocol::{LONG, SHORT};
use crate::hidraw::descriptor::{REPORT_ID, USAGE_PAGE, items};

const HIDPP_USAGE_PAGES: [u32; 2] = [0xFF00, 0xFF43];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reports {
    pub short: bool,
    pub long: bool,
}

impl Reports {
    pub fn any(self) -> bool {
        self.short || self.long
    }
}

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
    use crate::fake_hidraw::hex;

    #[test]
    fn hidpp_reports_are_found_on_the_vendor_page_only() {
        assert_eq!(reports(&hex("0600ff0901a1018510750895061500")), Reports { short: true, long: false });
        assert_eq!(reports(&hex("0600ff8510851185")), Reports { short: true, long: true }, "a cut-off item ends it");
        assert_eq!(reports(&hex("05010902a1018510")), Reports::default());
        assert_eq!(reports(&hex("0643ff0a02028511")), Reports { short: false, long: true }, "Bluetooth's page");
        assert_eq!(reports(&[]), Reports::default());
    }
}
