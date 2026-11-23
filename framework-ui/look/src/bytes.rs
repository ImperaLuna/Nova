//! Sizes in bytes as people read them, for every list that shows one
//! (quick settings' network and resources pages). `ui/text/bytes.js` in
//! today's shell.

const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
/// A unit is shown from this many of the one below: "1000 B" reads as
/// "1.0 KB" (each unit holds 1024 of the one below).
const NEXT_UNIT_FROM: f64 = 1000.0;
const UNIT_SIZE: f64 = 1024.0;
/// Under this, one decimal.
const DECIMAL_UNDER: f64 = 10.0;

/// A size in bytes, short: "9.4G", "780M", "42K". One decimal under 10.
pub fn short_bytes(bytes: u64) -> String {
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= NEXT_UNIT_FROM && unit < UNITS.len() - 1 {
        value /= UNIT_SIZE;
        unit += 1;
    }
    let digits = usize::from(value < DECIMAL_UNDER && unit > 0);
    format!("{value:.digits$}{}", UNITS[unit])
}

/// A size in bytes for a list: "2.4 GB", "310 MB", "12 B".
pub fn bytes(bytes: u64) -> String {
    let short = short_bytes(bytes);
    let (number, unit) = short.split_at(short.len() - 1);
    match unit {
        "B" => format!("{number} B"),
        unit => format!("{number} {unit}B"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_as_bytes_js_writes_them() {
        assert_eq!(short_bytes(0), "0B");
        assert_eq!(short_bytes(999), "999B");
        assert_eq!(short_bytes(1000), "1.0K");
        assert_eq!(short_bytes(43_008), "42K");
        assert_eq!(short_bytes(817_889_280), "780M");
        assert_eq!(short_bytes(10_093_173_145), "9.4G");
        assert_eq!(bytes(12), "12 B");
        assert_eq!(bytes(2_576_980_378), "2.4 GB");
        assert_eq!(bytes(325_058_560), "310 MB");
    }
}
