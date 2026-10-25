//! What the machine is busy with (feature `quick-settings`): CPU, memory,
//! GPU, the CPU's temperature and the apps using the most memory, for the quick
//! settings sidebar's resources card and page.
//!
//! One thread samples. CPU and memory go into a short history every
//! [`RESOURCE_HISTORY_INTERVAL_SECS`], always, so the page's graph has the
//! last minutes as soon as it opens (two small reads from `/proc`). The
//! rest (every process, sensors, NVML or `nvidia-smi`) is read only while some
//! connection watches (`watch_resources`), every [`LIVE_INTERVAL`], and
//! each look is published then. A connection that closes stops watching.

mod apps;
mod nvidia;
mod nvidia_smi;
mod sensors;
mod usage;

use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use apps::Me;
use nova_protocol::{Event, RESOURCE_HISTORY_INTERVAL_SECS, ResourceSample, Resources as Reading};
use sensors::{GpuBusy, Temperature};
use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use usage::{CpuTimes, ProcessTicks};

pub const FEATURE_ID: &str = "quick-settings";
const LOG_SOURCE: &str = "resources";

/// How often a watched look is taken and published.
pub(crate) const LIVE_INTERVAL: Duration = Duration::from_secs(2);
/// The first look after watching starts, soon enough to show at once but
/// far enough from the baseline for CPU shares to mean something.
const FIRST_LOOK: Duration = Duration::from_millis(250);
const HISTORY_INTERVAL: Duration = Duration::from_secs(RESOURCE_HISTORY_INTERVAL_SECS);
/// How far back the history goes.
const HISTORY_SPAN: Duration = Duration::from_secs(3 * 60);
/// How many apps a look lists (and Nova, wherever it ranks): enough to
/// scroll through, few enough to send every two seconds.
const TOP_APPS: usize = 25;

/// Where readings come from: the real `/proc` and `/sys`, or recorded
/// copies in tests.
#[derive(Clone)]
pub struct Sources {
    pub proc_root: PathBuf,
    pub sys_root: PathBuf,
    /// Where NVIDIA's NVML library is looked for.
    pub nvml: Vec<PathBuf>,
    /// Searched for `nvidia-smi` where NVML doesn't load.
    pub path_var: String,
}

impl Sources {
    pub fn system(path_var: &str) -> Self {
        Self {
            proc_root: "/proc".into(),
            sys_root: "/sys".into(),
            nvml: nvidia::library_paths(),
            path_var: path_var.into(),
        }
    }
}

/// Where an NVIDIA GPU's use comes from, when the driver's sysfs doesn't say.
enum NvidiaSource {
    Nvml(nvidia::Library),
    Smi(PathBuf),
}

/// That source while resources are watched.
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
    /// Where `end_app` finds processes.
    proc_root: PathBuf,
    state: Mutex<State>,
    /// Wakes the sampler when watching starts or stops, or it should stop.
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

    /// Ends the sampler; nothing is published after this returns.
    pub fn stop(&self) {
        lock(&self.state).stopped = true;
        self.wake.notify_all();
    }

    /// Starts or stops `client` watching.
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

    /// Asks every process of the app `key` (an `AppUsage::key`) to close.
    pub fn end_app(&self, key: &str) -> Result<(), String> {
        apps::end(&usage::processes(&self.proc_root), key, Me::current()).map(|_| ())
    }

    fn run(&self, mut sampler: Sampler) {
        let mut next_history = Instant::now();
        // When the next watched look is due, while anyone watches.
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
                // Published under the state lock: a `stop` or the last
                // watcher leaving can't be overtaken by this look.
                self.hub.publish(&Event::Resources(reading));
                next_look = Some(due.max(now) + LIVE_INTERVAL);
            }
            let until = next_look.map_or(next_history, |look| look.min(next_history));
            let timeout = until.saturating_duration_since(Instant::now());
            state = self.wake.wait_timeout(state, timeout).unwrap_or_else(std::sync::PoisonError::into_inner).0;
        }
    }
}

/// The sources and what each needs from the last look.
struct Sampler {
    sources: Sources,
    temperature: Option<Temperature>,
    gpu_busy: Option<GpuBusy>,
    nvidia_source: Option<NvidiaSource>,
    logger: Arc<Logger>,
    history: VecDeque<ResourceSample>,
    /// CPU times at the last history sample, and at the last watched look.
    history_cpu: Option<CpuTimes>,
    look_cpu: Option<CpuTimes>,
    process_ticks: ProcessTicks,
    /// Started while watched, when the GPU is NVIDIA's.
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
                    .map(NvidiaSource::Nvml)
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

    /// Adds CPU and memory now to the history.
    fn record_history(&mut self) {
        let cpu_now = CpuTimes::read(&self.sources.proc_root);
        let cpu = match (cpu_now, self.history_cpu) {
            (Some(now), Some(before)) => now.busy_since(&before),
            _ => 0.0,
        };
        let had_baseline = self.history_cpu.is_some();
        self.history_cpu = cpu_now;
        // The first sample only sets the baseline.
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
            Some(NvidiaSource::Nvml(library)) => library
                .start()
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

    /// Everything now, compared with the last look.
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

    /// The apps using the most memory, with their CPU use since the last
    /// look, `total_ticks` being all CPUs' time over it.
    fn top_apps(&mut self, total_ticks: u64) -> Vec<nova_protocol::AppUsage> {
        let processes = usage::processes(&self.sources.proc_root);
        let used = self.process_ticks.since_last(&processes);
        let proc_root = &self.sources.proc_root;
        apps::top(&processes, &used, total_ticks, page_size(), Me::current(), TOP_APPS, |pid| {
            usage::read_pss(proc_root, pid)
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
    use nova_protocol::LogLevel;

    fn logger(name: &str) -> Arc<Logger> {
        let dir = std::env::temp_dir().join(format!("novad-resources-{name}-{}", std::process::id()));
        Arc::new(Logger::open(&dir, LogLevel::Debug, false, Arc::new(Hub::default())).unwrap())
    }

    #[test]
    fn a_look_reads_this_machine() {
        let mut sampler = Sampler::new(Sources::system(""), &logger("look"));
        sampler.record_history();
        sampler.begin_watching();
        // Some CPU time to count.
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
