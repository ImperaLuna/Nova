use nova_look::bytes::short_bytes;
use nova_resources_api::{AppUsage, Resources};

pub const DASH: &str = "—";

#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    pub label: &'static str,
    pub value: f32,
    pub reading: String,
}

pub fn percent(value: f32) -> String {
    format!("{}%", value.round())
}

pub fn share(used: u64, total: u64) -> f32 {
    if total == 0 { 0.0 } else { (used as f64 / total as f64).clamp(0.0, 1.0) as f32 }
}

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

pub fn matching<'a>(apps: &'a [AppUsage], query: &str) -> Vec<&'a AppUsage> {
    let query = query.trim().to_lowercase();
    apps.iter().filter(|app| query.is_empty() || app.name.to_lowercase().contains(&query)).collect()
}

pub fn processes(count: u32) -> Option<String> {
    (count > 1).then(|| format!("×{count}"))
}

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
        assert_eq!(names("FOO"), ["foot"]);
        assert_eq!(names("disc "), ["Discord"]);
        assert!(names("zzz").is_empty());
    }

    #[test]
    fn numbers_on_a_row() {
        assert_eq!(processes(1), None);
        assert_eq!(processes(9).as_deref(), Some("×9"));
        assert_eq!(cpu(6.14), "6.1%");
    }
}
