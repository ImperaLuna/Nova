//! Reading a HID report descriptor, which says what reports a hidraw node
//! takes and what they're for. It's a list of items, each a prefix byte
//! (tag, type and data size) and up to 4 bytes of data, or a long item
//! (`0xFE`, its data size, its tag, then the data), which is skipped. The
//! sources only need a few: Usage Page and Report ID (global items) and
//! Usage (local).

/// Items by their prefix with the size bits off: Usage Page and Report ID
/// (global), Usage (local).
pub const USAGE_PAGE: u8 = 0x04;
pub const REPORT_ID: u8 = 0x84;
pub const USAGE: u8 = 0x08;
/// A long item's prefix, followed by its data size and tag.
const LONG_ITEM: u8 = 0xFE;
/// The data sizes a short item's two low bits stand for.
const DATA_SIZES: [usize; 4] = [0, 1, 2, 4];
/// A 4-byte Usage holds its own usage page in its high half.
const EXTENDED_USAGE_BYTES: usize = 4;

/// One short item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    /// Its prefix with the size bits off, like [`USAGE_PAGE`].
    pub tag: u8,
    pub value: u32,
    /// How many bytes of data it had.
    pub size: usize,
}

/// The short items of `descriptor`, in order, up to the first cut-off one.
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

/// Whether `descriptor` has usage `usage` on usage page `page` (a
/// collection or report for it).
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
pub fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_read_in_order_and_long_ones_skipped() {
        // Usage Page 0xFF60, a long item, Usage 0x61, then a cut-off Report ID.
        let found: Vec<(u8, u32)> = items(&hex("0660fffe0200aaaa096185")).map(|item| (item.tag, item.value)).collect();
        assert_eq!(found, [(USAGE_PAGE, 0xFF60), (USAGE, 0x61)]);
        assert_eq!(items(&[]).count(), 0);
    }

    #[test]
    fn a_usage_counts_on_its_own_page_only() {
        // QMK's raw HID: Usage Page 0xFF60, Usage 0x61, Collection.
        assert!(has_usage(&hex("0660ff0961a101"), 0xFF60, 0x61));
        // A keyboard's Usage 0x61 on the Generic Desktop page isn't it.
        assert!(!has_usage(&hex("05010961a101"), 0xFF60, 0x61));
        // A 4-byte Usage names its page itself.
        assert!(has_usage(&hex("0b610060ff"), 0xFF60, 0x61));
    }
}
