use super::*;

#[derive(Default)]
pub(crate) struct Shown {
    pub(crate) outputs: BTreeMap<String, PathBuf>,
    pub(crate) unfollowed: Option<Choice<PathBuf>>,
}

pub(crate) struct Awww {
    pub(crate) shown: Shown,
    pub(crate) restarts: Restarts,
    pub(crate) start_at: Option<Instant>,
    pub(crate) resend: Backoff,
    pub(crate) resend_at: Option<Instant>,
    pub(crate) starts: u64,
    pub(crate) sizes: BTreeMap<String, (u32, u32)>,
}

#[derive(Default)]
pub(crate) struct EngineRun {
    pub(crate) shown: Option<BTreeMap<String, PathBuf>>,
    pub(crate) restarts: Restarts,
    pub(crate) start_at: Option<Instant>,
    pub(crate) above: u64,
    pub(crate) frame: Option<Frame>,
}

pub(crate) struct Frame {
    pub(crate) screens: BTreeMap<String, PathBuf>,
    pub(crate) until: Instant,
}

pub(crate) fn show(wanted: &Choice<PathBuf>, outputs: &Outputs, shown: &mut Shown, drive: Drive) -> bool {
    let Drive { logger, stopped, .. } = drive;
    let path_var = drive.places.path_var.as_str();
    match outputs {
        Outputs::Unknown => true,
        Outputs::Known(names) => {
            let mut changes: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
            for (name, path) in wanted.per_output(names) {
                if shown.outputs.get(&name) != Some(&path) {
                    changes.entry(path).or_default().push(name);
                }
            }
            let mut all_shown = true;
            for (path, names) in changes {
                if stopped() {
                    return true;
                }
                match send_image(&path, Some(&names), path_var) {
                    Ok(()) => shown.outputs.extend(names.into_iter().map(|name| (name, path.clone()))),
                    Err(e) => {
                        logger.warn(LOG_SOURCE, &format!("can't show {}: {e}", path.display()));
                        all_shown = false;
                    }
                }
            }
            all_shown
        }
        Outputs::Unfollowed => {
            if shown.unfollowed.as_ref() == Some(wanted) {
                return true;
            }
            let every = wanted.path.iter().map(|path| (path, None));
            let own = wanted.outputs.iter().map(|(name, path)| (path, Some(vec![name.clone()])));
            let mut all_shown = true;
            for (path, names) in every.chain(own) {
                if stopped() {
                    return true;
                }
                if let Err(e) = send_image(path, names.as_deref(), path_var) {
                    logger.warn(LOG_SOURCE, &format!("can't show {}: {e}", path.display()));
                    all_shown = false;
                }
            }
            shown.unfollowed = all_shown.then(|| wanted.clone());
            all_shown
        }
    }
}

pub(crate) fn query(path_var: &str) -> std::io::Result<std::process::Output> {
    let mut query = awww(path_var);
    query.args(["query", "--namespace", NAMESPACE]);
    child::output_within(query, QUERY_TIMEOUT)
}

pub(crate) fn query_sizes(path_var: &str) -> Option<BTreeMap<String, (u32, u32)>> {
    let output = query(path_var).ok().filter(|output| output.status.success())?;
    Some(sizes(&String::from_utf8_lossy(&output.stdout)))
}

pub(crate) fn sizes(answer: &str) -> BTreeMap<String, (u32, u32)> {
    let size = |line: &str| {
        let (front, back) = line.split_once(", scale: ")?;
        let scale: f64 = back.split(',').next()?.trim().parse().ok()?;
        let (front, dim) = front.rsplit_once(": ")?;
        let name = front.rsplit_once(": ").map_or(front, |(_, name)| name);
        let (width, height) = dim.split_once('x')?;
        let pixels = |length: &str| Some((f64::from(length.parse::<u32>().ok()?) * scale).round() as u32);
        Some((name.to_owned(), (pixels(width)?, pixels(height)?))).filter(|(_, (width, height))| width * height > 0)
    };
    answer.lines().filter_map(size).collect()
}

pub(crate) fn keep_stills(engine: &mut EngineRun, written: bool, sizes: &BTreeMap<String, (u32, u32)>, drive: Drive) {
    let Drive { places, logger, stopped } = drive;
    let Some(frame) = engine.frame.as_ref() else {
        return;
    };
    let file = engine::frame_file(&places.engine);
    let saved = match written {
        false => Err(engine::Unsaved::NotYet),
        true => match std::fs::read(&file) {
            Ok(bytes) => engine::save_stills(&bytes, &frame.screens, sizes, &places.engine, stopped),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(engine::Unsaved::NotYet),
            Err(e) => Err(engine::Unsaved::Never(format!("can't read {}: {e}", file.display()))),
        },
    };
    let why = match saved {
        Err(engine::Unsaved::NotYet) if Instant::now() < frame.until => return,
        Ok(()) => None,
        Err(engine::Unsaved::NotYet) => Some("it didn't write one".to_owned()),
        Err(engine::Unsaved::Never(why)) => Some(why),
    };
    match why {
        None => logger.log(LogLevel::Debug, LOG_SOURCE, "kept stills of the engine's frame"),
        Some(why) => logger.info(LOG_SOURCE, &format!("no stills from the engine's frame: {why}")),
    }
    engine.frame = None;
    if !stopped() {
        let _ = std::fs::remove_file(&file);
    }
}

pub(crate) fn send_image(path: &Path, outputs: Option<&[String]>, path_var: &str) -> Result<(), String> {
    let mut command = awww(path_var);
    command.args([
        "img",
        "--namespace",
        NAMESPACE,
        "--transition-type",
        TRANSITION,
        "--transition-duration",
        TRANSITION_SECONDS,
        "--transition-fps",
        TRANSITION_FPS,
        "--resize",
        RESIZE,
    ]);
    if let Some(outputs) = outputs {
        command.arg("--outputs").arg(outputs.join(","));
    }
    command.arg(path);
    let output = child::output_within(command, IMAGE_TIMEOUT).map_err(|e| e.to_string())?;
    match output.status.success() {
        true => Ok(()),
        false => Err(format!("{CLIENT} {}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim())),
    }
}

pub(crate) fn awww(path_var: &str) -> Command {
    let mut command = Command::new(CLIENT);
    command.env("PATH", path_var);
    command
}
