const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
const NEXT_UNIT_FROM: f64 = 1000.0;
const UNIT_SIZE: f64 = 1024.0;
const DECIMAL_UNDER: f64 = 10.0;

pub fn short_bytes(bytes: u64) -> String {
    let mut value = bytes as f64;
    let mut unit = 0;
    while rounded(value, unit) >= NEXT_UNIT_FROM && unit < UNITS.len() - 1 {
        value /= UNIT_SIZE;
        unit += 1;
    }
    let digits = decimals(value, unit);
    format!("{value:.digits$}{}", UNITS[unit])
}

fn decimals(value: f64, unit: usize) -> usize {
    usize::from(unit > 0 && (value * 10.0).round() / 10.0 < DECIMAL_UNDER)
}

fn rounded(value: f64, unit: usize) -> f64 {
    let scale = 10f64.powi(decimals(value, unit) as i32);
    (value * scale).round() / scale
}

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
        assert_eq!(short_bytes(1_023_950), "1.0M");
        assert_eq!(short_bytes(10_199), "10K");
        assert_eq!(bytes(12), "12 B");
        assert_eq!(bytes(2_576_980_378), "2.4 GB");
        assert_eq!(bytes(325_058_560), "310 MB");
    }
}
