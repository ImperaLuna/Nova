const MINUTE_SECONDS: i64 = 60;
const HOUR_MINUTES: i64 = 60;
pub const DAY_MINUTES: i64 = 24 * HOUR_MINUTES;

pub fn minutes(minutes: i64) -> String {
    let (days, hours, rest) = (minutes / DAY_MINUTES, minutes % DAY_MINUTES / HOUR_MINUTES, minutes % HOUR_MINUTES);
    match (days, hours, rest) {
        (0, 0, rest) => format!("{rest}m"),
        (0, hours, 0) => format!("{hours}h"),
        (0, hours, rest) => format!("{hours}h {rest}m"),
        (days, 0, _) => format!("{days}d"),
        (days, hours, _) => format!("{days}d {hours}h"),
    }
}

pub fn uptime(seconds: i64) -> String {
    format!("up {}", minutes(seconds.max(0) / MINUTE_SECONDS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_read_as_duration_js_writes_them() {
        assert_eq!(minutes(45), "45m");
        assert_eq!(minutes(120), "2h");
        assert_eq!(minutes(134), "2h 14m");
        assert_eq!(minutes(3 * DAY_MINUTES), "3d");
        assert_eq!(minutes(3 * DAY_MINUTES + 2 * 60 + 5), "3d 2h");
    }

    #[test]
    fn the_uptime_counts_whole_minutes_down() {
        assert_eq!(uptime(59), "up 0m");
        assert_eq!(uptime(45 * 60 + 59), "up 45m");
        assert_eq!(uptime(2 * 3600), "up 2h");
        assert_eq!(uptime(2 * 3600 + 14 * 60), "up 2h 14m");
        assert_eq!(uptime(3 * 86_400), "up 3d");
        assert_eq!(uptime(3 * 86_400 + 2 * 3600 + 5 * 60), "up 3d 2h");
        assert_eq!(uptime(-5), "up 0m", "a clock that went back");
    }
}
