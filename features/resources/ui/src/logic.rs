//! What `ResourcesCard.qml`, `ResourcesPage.qml`, `Graph.qml` and the `.js`
//! they use worked out, with no GPUI drawing in it: the rings' readings,
//! sizes as people read them, the apps a search finds, and where the
//! graph's points go.

use nova_resources_api::{AppUsage, Resources};

/// What a reading novad doesn't have shows.
pub const DASH: &str = "—";

/// One ring on the card: its name, how full it is (0 to 1) and the text in
/// it.
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    pub label: &'static str,
    pub value: f32,
    pub reading: String,
}

/// "23%".
pub fn percent(value: f32) -> String {
    format!("{}%", value.round())
}

/// `used` as a share of `total`, 0 to 1.
pub fn share(used: u64, total: u64) -> f32 {
    if total == 0 { 0.0 } else { (used as f64 / total as f64).clamp(0.0, 1.0) as f32 }
}

/// A size in bytes, short: "9.4G", "780M", "42K". One decimal under 10.
pub fn short_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let digits = if value < 10.0 && unit > 0 { 1 } else { 0 };
    format!("{value:.digits$}{}", UNITS[unit])
}

/// A size in bytes for a list: "2.4 GB", "310 MB".
pub fn bytes(bytes: u64) -> String {
    let short = short_bytes(bytes);
    let (number, unit) = short.split_at(short.len() - 1);
    if unit == "B" { format!("{number} B") } else { format!("{number} {unit}B") }
}

/// CPU, memory, GPU and the CPU's temperature, a dash for what novad
/// doesn't have.
pub fn rings(now: Option<&Resources>) -> [Ring; 4] {
    let dash = || DASH.to_owned();
    let ring = |label, value: Option<f32>, reading: Option<String>| Ring {
        label,
        value: value.unwrap_or(0.0),
        reading: reading.unwrap_or_else(dash),
    };
    [
        ring("CPU", now.map(|now| now.cpu / 100.0), now.map(|now| percent(now.cpu))),
        ring(
            "RAM",
            now.map(|now| share(now.memory_used, now.memory_total)),
            now.map(|now| short_bytes(now.memory_used)),
        ),
        ring("GPU", now.and_then(|now| now.gpu).map(|gpu| gpu / 100.0), now.and_then(|now| now.gpu).map(percent)),
        ring(
            "Temp",
            now.and_then(|now| now.temperature).map(|degrees| degrees / 100.0),
            now.and_then(|now| now.temperature).map(|degrees| format!("{}°", degrees.round())),
        ),
    ]
}

/// The apps whose name has `query`, any case; all of them for none.
pub fn matching<'a>(apps: &'a [AppUsage], query: &str) -> Vec<&'a AppUsage> {
    let query = query.trim().to_lowercase();
    apps.iter().filter(|app| query.is_empty() || app.name.to_lowercase().contains(&query)).collect()
}

/// Where `values` (0 to 100, oldest first) go across a graph `width` by
/// `height` that `count` samples fill: newest at the right edge, each a
/// step to the left of the next.
pub fn graph_points(values: &[f32], width: f32, height: f32, count: usize) -> Vec<(f32, f32)> {
    let step = width / count.saturating_sub(1).max(1) as f32;
    let start = width - step * values.len().saturating_sub(1) as f32;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| (start + step * index as f32, height - value.clamp(0.0, 100.0) / 100.0 * height))
        .collect()
}

/// "×3" beside an app that runs more than one process.
pub fn processes(count: u32) -> Option<String> {
    (count > 1).then(|| format!("×{count}"))
}

/// An app's CPU share with one decimal: "6.1%".
pub fn cpu(share: f32) -> String {
    format!("{share:.1}%")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> AppUsage {
        AppUsage { name: name.into(), key: name.into(), processes: 1, cpu: 0.0, memory: 0, protected: false }
    }

    #[test]
    fn sizes_read_as_people_read_them() {
        assert_eq!(short_bytes(42), "42B");
        assert_eq!(short_bytes(43_008), "42K");
        assert_eq!(short_bytes(817_889_280), "780M");
        assert_eq!(short_bytes(10_093_173_760), "9.4G");
        assert_eq!(bytes(2_576_980_378), "2.4 GB");
        assert_eq!(bytes(325_058_560), "310 MB");
        assert_eq!(bytes(12), "12 B");
    }

    #[test]
    fn rings_show_a_dash_for_what_novad_lacks() {
        let none = rings(None);
        assert!(none.iter().all(|ring| ring.reading == DASH && ring.value == 0.0));
        let now = Resources {
            cpu: 23.5,
            memory_used: 8 << 30,
            memory_total: 32 << 30,
            gpu: None,
            temperature: Some(54.4),
            history: Vec::new(),
            history_size: 36,
            apps: Vec::new(),
        };
        let [cpu, ram, gpu, temp] = rings(Some(&now));
        assert_eq!((cpu.reading.as_str(), cpu.value), ("24%", 0.235));
        assert_eq!((ram.reading.as_str(), ram.value), ("8.0G", 0.25));
        assert_eq!((gpu.reading.as_str(), gpu.value), (DASH, 0.0));
        assert_eq!(temp.reading, "54°");
    }

    #[test]
    fn search_finds_apps_by_name_in_any_case() {
        let apps = [app("firefox"), app("Discord"), app("foot")];
        let names = |query| matching(&apps, query).iter().map(|app| app.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names(""), ["firefox", "Discord", "foot"]);
        assert_eq!(names("FO"), ["foot"]);
        assert_eq!(names("disc "), ["Discord"]);
        assert!(names("zzz").is_empty());
    }

    #[test]
    fn the_newest_sample_is_at_the_right_edge() {
        let points = graph_points(&[0.0, 50.0, 150.0], 100.0, 10.0, 11);
        assert_eq!(points, [(80.0, 10.0), (90.0, 5.0), (100.0, 0.0)]);
        assert!(graph_points(&[], 100.0, 10.0, 11).is_empty());
        assert_eq!(graph_points(&[20.0], 100.0, 10.0, 1), [(100.0, 8.0)]);
    }

    #[test]
    fn numbers_on_a_row() {
        assert_eq!(processes(1), None);
        assert_eq!(processes(9).as_deref(), Some("×9"));
        assert_eq!(cpu(6.14), "6.1%");
    }
}
