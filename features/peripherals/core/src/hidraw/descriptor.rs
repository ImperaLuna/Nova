pub const USAGE_PAGE: u8 = 0x04;
pub const REPORT_ID: u8 = 0x84;
pub const USAGE: u8 = 0x08;
const LONG_ITEM: u8 = 0xFE;
const DATA_SIZES: [usize; 4] = [0, 1, 2, 4];
const EXTENDED_USAGE_BYTES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub tag: u8,
    pub value: u32,
    pub size: usize,
}

pub fn items(descriptor: &[u8]) -> impl Iterator<Item = Item> + '_ {
    let mut at = 0;
    std::iter::from_fn(move || {
        loop {
            let prefix = *descriptor.get(at)?;
            if prefix == LONG_ITEM {
                let size = descriptor.get(at + 1).copied().map_or(0, usize::from);
                at += 3 + size;
                continue;
            }
            let size = DATA_SIZES[usize::from(prefix & 0x03)];
            let data = descriptor.get(at + 1..at + 1 + size)?;
            at += 1 + size;
            let value = data.iter().rev().fold(0u32, |value, byte| (value << 8) | u32::from(*byte));
            return Some(Item { tag: prefix & 0xFC, value, size });
        }
    })
}

pub fn has_usage(descriptor: &[u8], page: u16, usage: u16) -> bool {
    let mut current_page = 0;
    items(descriptor).any(|item| match item.tag {
        USAGE_PAGE => {
            current_page = item.value;
            false
        }
        USAGE if item.size == EXTENDED_USAGE_BYTES => item.value == (u32::from(page) << 16 | u32::from(usage)),
        USAGE => current_page == u32::from(page) && item.value == u32::from(usage),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_hidraw::hex;

    #[test]
    fn items_are_read_in_order_and_long_ones_skipped() {
        let found: Vec<(u8, u32)> = items(&hex("0660fffe0200aaaa096185")).map(|item| (item.tag, item.value)).collect();
        assert_eq!(found, [(USAGE_PAGE, 0xFF60), (USAGE, 0x61)]);
        assert_eq!(items(&[]).count(), 0);
    }

    #[test]
    fn a_usage_counts_on_its_own_page_only() {
        assert!(has_usage(&hex("0660ff0961a101"), 0xFF60, 0x61));
        assert!(!has_usage(&hex("05010961a101"), 0xFF60, 0x61));
        assert!(has_usage(&hex("0b610060ff"), 0xFF60, 0x61));
    }
}
