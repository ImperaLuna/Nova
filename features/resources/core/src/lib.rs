mod apps;
mod nvidia;
mod nvidia_smi;
mod sensors;
mod usage;

use apps::Me;
use nova_base::lock;
use nova_hub::{ClientId, Hub};
use nova_log::Logger;
use nova_process::procfs;
use nova_resources_api::{AppUsage, Event, RESOURCE_HISTORY_INTERVAL_SECS, ResourceSample, Resources as Reading};
use sensors::{GpuBusy, Temperature};
use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use usage::{CpuTimes, ProcessTicks};

pub const FEATURE_ID: &str = "resources";
const LOG_SOURCE: &str = "resources";

pub const LIVE_INTERVAL: Duration = Duration::from_secs(2);
const FIRST_LOOK: Duration = Duration::from_millis(250);
const HISTORY_INTERVAL: Duration = Duration::from_secs(RESOURCE_HISTORY_INTERVAL_SECS);
const HISTORY_SPAN: Duration = Duration::from_secs(3 * 60);
const TOP_APPS: usize = 25;

#[derive(Clone)]
pub struct Sources {
    pub proc_root: PathBuf,
    pub sys_root: PathBuf,
    pub nvml: Vec<PathBuf>,
    pub path_var: String,
}

impl Sources {
    pub fn system(path_var: &str) -> Self {
        Self {
            proc_root: procfs::PROC.into(),
            sys_root: "/sys".into(),
            nvml: nvidia::library_paths(),
            path_var: path_var.into(),
        }
    }
}

enum NvidiaSource {
    Nvml(Vec<PathBuf>),
    Smi(PathBuf),
}

enum NvidiaReader {
    Nvml(nvidia::Nvml),
    Smi(nvidia_smi::NvidiaSmi),
}

impl NvidiaReader {
    fn busy(&self) -> Option<f32> {
        match self {
            Self::Nvml(nvml) => nvml.busy(),
            Self::Smi(smi) => smi.latest(),
        }
    }
}

pub struct Resources {
    proc_root: PathBuf,
    state: Mutex<State>,
    wake: Condvar,
    hub: Arc<Hub>,
}

struct State {
    watchers: BTreeSet<ClientId>,
    stopped: bool,
}

impl Resources {
    pub fn start(sources: Sources, hub: Arc<Hub>, logger: Arc<Logger>) -> Arc<Self> {
        let resources = Arc::new(Self {
            proc_root: sources.proc_root.clone(),
            state: Mutex::new(State { watchers: BTreeSet::new(), stopped: false }),
            wake: Condvar::new(),
            hub,
        });
        let sampler = Arc::clone(&resources);
        std::thread::spawn(move || sampler.run(Sampler::new(sources, &logger)));
        resources
    }

    pub fn stop(&self) {
        lock(&self.state).stopped = true;
        self.wake.notify_all();
    }

    pub fn watch(&self, client: ClientId, watching: bool) {
        let mut state = lock(&self.state);
        let changed = match watching {
            true => state.watchers.insert(client),
            false => state.watchers.remove(&client),
        };
        drop(state);
        if changed {
            self.wake.notify_all();
        }
    }

    pub fn end_app(&self, key: &str) -> Result<(), String> {
        apps::end(&usage::processes(&self.proc_root), key, Me::current()).map(|_| ())
    }

    fn run(&self, mut sampler: Sampler) {
        let mut next_history = Instant::now();
        let mut next_look: Option<Instant> = None;
        let mut state = lock(&self.state);
        loop {
            if state.stopped {
                return;
            }
            let watched = !state.watchers.is_empty();
            match (watched, next_look) {
                (true, None) => {
                    sampler.begin_watching();
                    next_look = Some(Instant::now() + FIRST_LOOK);
                }
                (false, Some(_)) => {
                    sampler.end_watching();
                    next_look = None;
                }
                _ => {}
            }
            let now = Instant::now();
            if now >= next_history {
                sampler.record_history();
                next_history = now + HISTORY_INTERVAL;
            }
            if let Some(due) = next_look.filter(|due| now >= *due) {
                let reading = sampler.look();
                self.hub.publish(&Event::Resources(reading));
                next_look = Some(due.max(now) + LIVE_INTERVAL);
            }
            let until = next_look.map_or(next_history, |look| look.min(next_history));
            let timeout = until.saturating_duration_since(Instant::now());
            state = self.wake.wait_timeout(state, timeout).unwrap_or_else(std::sync::PoisonError::into_inner).0;
        }
    }
}

struct Sampler {
    sources: Sources,
    temperature: Option<Temperature>,
    gpu_busy: Option<GpuBusy>,
    nvidia_source: Option<NvidiaSource>,
    logger: Arc<Logger>,
    history: VecDeque<ResourceSample>,
    history_cpu: Option<CpuTimes>,
    look_cpu: Option<CpuTimes>,
    process_ticks: ProcessTicks,
    nvidia: Option<NvidiaReader>,
}

impl Sampler {
    fn new(sources: Sources, logger: &Arc<Logger>) -> Self {
        let temperature = sensors::Temperature::find(&sources.sys_root);
        let gpu_busy = sensors::GpuBusy::find(&sources.sys_root);
        let nvidia_source = gpu_busy
            .is_none()
            .then(|| {
                nvidia::Library::open(&sources.nvml)
                    .map(|_| NvidiaSource::Nvml(sources.nvml.clone()))
                    .or_else(|| nvidia_smi::find(&sources.path_var).map(NvidiaSource::Smi))
            })
            .flatten();
        let gpu = match (&gpu_busy, &nvidia_source) {
            (Some(_), _) => "from the driver",
            (None, Some(NvidiaSource::Nvml(_))) => "from NVML",
            (None, Some(NvidiaSource::Smi(_))) => "from nvidia-smi",
            (None, None) => "not reported",
        };
        let temperature_found = if temperature.is_some() { "found" } else { "not found" };
        logger.info(LOG_SOURCE, &format!("GPU use {gpu}; CPU temperature sensor {temperature_found}"));
        Self {
            sources,
            temperature,
            gpu_busy,
            nvidia_source,
            logger: Arc::clone(logger),
            history: VecDeque::new(),
            history_cpu: None,
            look_cpu: None,
            process_ticks: ProcessTicks::default(),
            nvidia: None,
        }
    }

    fn history_len() -> usize {
        (HISTORY_SPAN.as_secs() / HISTORY_INTERVAL.as_secs()) as usize
    }

    fn record_history(&mut self) {
        let cpu_now = CpuTimes::read(&self.sources.proc_root);
        let cpu = match (cpu_now, self.history_cpu) {
            (Some(now), Some(before)) => now.busy_since(&before),
            _ => 0.0,
        };
        let had_baseline = self.history_cpu.is_some();
        self.history_cpu = cpu_now;
        if !had_baseline {
            return;
        }
        let memory =
            usage::read_memory(&self.sources.proc_root).map_or(0.0, |(used, total)| usage::percent(used, total));
        self.history.push_back(ResourceSample { cpu, memory });
        while self.history.len() > Self::history_len() {
            self.history.pop_front();
        }
    }

    fn begin_watching(&mut self) {
        self.look_cpu = CpuTimes::read(&self.sources.proc_root);
        self.process_ticks.since_last(&usage::processes(&self.sources.proc_root));
        self.nvidia = match &self.nvidia_source {
            Some(NvidiaSource::Nvml(paths)) => nvidia::Library::open(paths)
                .ok_or_else(|| "it didn't load again".to_string())
                .and_then(nvidia::Library::start)
                .map(NvidiaReader::Nvml)
                .map_err(|e| self.logger.warn(LOG_SOURCE, &format!("couldn't start NVML: {e}")))
                .ok(),
            Some(NvidiaSource::Smi(binary)) => {
                nvidia_smi::NvidiaSmi::start(binary, &self.sources.path_var, LIVE_INTERVAL)
                    .map(NvidiaReader::Smi)
                    .map_err(|e| self.logger.warn(LOG_SOURCE, &format!("couldn't run {}: {e}", binary.display())))
                    .ok()
            }
            None => None,
        };
    }

    fn end_watching(&mut self) {
        self.nvidia = None;
    }

    fn look(&mut self) -> Reading {
        let cpu_now = CpuTimes::read(&self.sources.proc_root);
        let total_ticks = match (cpu_now, self.look_cpu) {
            (Some(now), Some(before)) => now.total.saturating_sub(before.total),
            _ => 0,
        };
        let cpu = match (cpu_now, self.look_cpu) {
            (Some(now), Some(before)) => now.busy_since(&before),
            _ => 0.0,
        };
        self.look_cpu = cpu_now;
        let (memory_used, memory_total) = usage::read_memory(&self.sources.proc_root).unwrap_or((0, 0));
        Reading {
            cpu,
            memory_used,
            memory_total,
            gpu: self.gpu_busy.as_ref().and_then(GpuBusy::read).or_else(|| self.nvidia.as_ref()?.busy()),
            temperature: self.temperature.as_ref().and_then(Temperature::read),
            history: self.history.iter().copied().collect(),
            history_size: Self::history_len(),
            apps: self.top_apps(total_ticks),
        }
    }

    fn top_apps(&mut self, total_ticks: u64) -> Vec<AppUsage> {
        let processes = usage::processes(&self.sources.proc_root);
        let used = self.process_ticks.since_last(&processes);
        let proc_root = &self.sources.proc_root;
        let look = apps::Look { processes: &processes, used: &used, total_ticks };
        apps::top(look, page_size(), Me::current(), TOP_APPS, |pid| {
            procfs::pss_kib(proc_root, pid).map(|kib| kib * 1024)
        })
    }
}

fn page_size() -> u64 {
    // SAFETY: sysconf only reads a configuration value.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    u64::try_from(size).unwrap_or(4096)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_carrier::log::LogLevel;

    fn logger(name: &str) -> Arc<Logger> {
        let dir = nova_test_support::TempDir::new(&format!("resources-{name}"));
        Arc::new(Logger::open(&dir, LogLevel::Debug, false, Arc::new(Hub::default())).unwrap())
    }

    #[test]
    fn a_look_reads_this_machine() {
        let mut sampler = Sampler::new(Sources::system(""), &logger("look"));
        sampler.record_history();
        sampler.begin_watching();
        let busy_until = Instant::now() + Duration::from_millis(50);
        while Instant::now() < busy_until {
            std::hint::black_box(0);
        }
        let reading = sampler.look();
        assert!((0.0..=100.0).contains(&reading.cpu));
        assert!(reading.memory_used > 0 && reading.memory_used <= reading.memory_total);
        assert!(!reading.apps.is_empty() && reading.apps.len() <= TOP_APPS);
        assert!(reading.apps.windows(2).all(|pair| pair[0].memory >= pair[1].memory));
        assert!(reading.history.is_empty(), "the first history sample is only a baseline");
        sampler.record_history();
        assert_eq!(sampler.look().history.len(), 1);
    }

    #[test]
    fn history_keeps_the_last_few_minutes() {
        let mut sampler = Sampler::new(Sources::system(""), &logger("history"));
        for _ in 0..Sampler::history_len() + 5 {
            sampler.record_history();
        }
        assert_eq!(sampler.history.len(), Sampler::history_len());
    }
}
