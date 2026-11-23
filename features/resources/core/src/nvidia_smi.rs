use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const BINARY: &str = "nvidia-smi";

pub fn find(path_var: &str) -> Option<PathBuf> {
    nova_paths::find_program(BINARY, path_var)
}

pub struct NvidiaSmi {
    child: Option<Child>,
    latest: Arc<Mutex<Option<f32>>>,
}

impl NvidiaSmi {
    pub fn start(binary: &Path, path_var: &str, interval: Duration) -> std::io::Result<Self> {
        let mut child = Command::new(binary)
            .args(["--query-gpu=utilization.gpu", "--format=csv,noheader,nounits", "--id=0"])
            .arg(format!("--loop-ms={}", interval.as_millis()))
            .env_clear()
            .env("PATH", path_var)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("no stdout pipe"))?;
        let latest = Arc::new(Mutex::new(None));
        let shared = Arc::clone(&latest);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                *nova_base::lock(&shared) = parse(&line);
            }
        });
        Ok(Self { child: Some(child), latest })
    }

    pub fn latest(&self) -> Option<f32> {
        *nova_base::lock(&self.latest)
    }
}

impl Drop for NvidiaSmi {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            std::thread::spawn(move || child.wait());
        }
    }
}

fn parse(line: &str) -> Option<f32> {
    line.trim().parse::<f32>().ok().filter(|percent| (0.0..=100.0).contains(percent))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::wait_until;

    #[test]
    fn parses_its_lines() {
        assert_eq!(parse("18\n"), Some(18.0));
        assert_eq!(parse(" 100"), Some(100.0));
        assert_eq!(parse("[N/A]"), None);
        assert_eq!(parse("101"), None);
    }

    #[test]
    fn follows_what_it_prints_and_stops_with_it() {
        let dir = nova_test_support::TempDir::new("nvidia-smi");
        let script = nova_test_support::write_program(&dir, BINARY, "echo 7\necho 42\nexec sleep 60");
        let path_var = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default());
        assert_eq!(find(&path_var), Some(script.clone()));

        let smi = NvidiaSmi::start(&script, &path_var, Duration::from_secs(2)).unwrap();
        let pid = smi.child.as_ref().unwrap().id();
        wait_until(Duration::from_secs(5), "the second line", || smi.latest() == Some(42.0));
        drop(smi);
        wait_until(Duration::from_secs(5), "it to be stopped and reaped", || {
            !Path::new(&format!("/proc/{pid}")).exists()
        });
    }
}
