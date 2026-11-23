use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Picture {
    Image(PathBuf),
    Dynamic { project: PathBuf, preview: Option<PathBuf> },
}

impl Picture {
    pub(crate) fn image(&self, size: Option<(u32, u32)>, folder: &Path) -> Option<PathBuf> {
        match self {
            Self::Image(path) => Some(path.clone()),
            Self::Dynamic { project, preview } => {
                size.and_then(|size| engine::still(project, size, folder)).or_else(|| preview.clone())
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct Plan {
    pub(crate) images: Choice<Picture>,
    pub(crate) screens: BTreeMap<String, PathBuf>,
}

pub(crate) fn projects<'c>(
    kinds: &BTreeMap<&'c Source, Option<Kind>>,
    places: &Places,
    logger: &Logger,
    stopped: &dyn Fn() -> bool,
) -> BTreeMap<&'c Source, PathBuf> {
    let mut projects: BTreeMap<&Source, PathBuf> = BTreeMap::new();
    for (source, kind) in kinds {
        let project = match kind {
            Some(Kind::Project { folder, .. }) => Some(folder.clone()),
            Some(Kind::Video(video)) if !stopped() => engine::video_project(video, &places.engine)
                .map_err(|e| logger.warn(LOG_SOURCE, &format!("can't show {source}: {e}")))
                .ok(),
            _ => None,
        };
        if let Some(project) = project {
            projects.insert(source, project);
        }
    }
    let videos = kinds
        .iter()
        .filter(|(_, kind)| matches!(kind, Some(Kind::Video(_))))
        .filter_map(|(source, _)| projects.get(source).cloned())
        .collect();
    let used: Vec<&Path> = projects.values().map(PathBuf::as_path).collect();
    if !stopped()
        && let Err(e) = engine::forget_videos(&places.engine, &videos)
    {
        logger.warn(LOG_SOURCE, &e);
    }
    let dynamic = kinds.values().filter(|kind| kind.as_ref().is_none_or(Kind::is_dynamic)).count();
    if used.len() == dynamic
        && !stopped()
        && let Err(e) = engine::forget_stills(&places.engine, &used)
    {
        logger.warn(LOG_SOURCE, &e);
    }
    projects
}

impl Plan {
    pub(crate) fn new(
        choice: &Choice,
        outputs: &Outputs,
        (images_ready, engine_ready): (bool, bool),
        drive: Drive,
    ) -> Self {
        let Drive { places, logger, stopped } = drive;
        let steam_roots = places.steam_roots();
        let mut kinds: BTreeMap<&Source, Option<Kind>> = BTreeMap::new();
        for source in choice.all() {
            kinds.entry(source).or_insert_with(|| match source.kind(&steam_roots) {
                Ok(kind) if kind.is_dynamic() && !engine_ready => {
                    logger.warn(LOG_SOURCE, &format!("{source} can't move: {}", engine_not_ready()));
                    Some(kind)
                }
                Ok(kind) => Some(kind),
                Err(e) => {
                    logger.warn(LOG_SOURCE, &format!("can't show {source}: {e}"));
                    None
                }
            });
        }
        let projects = if engine_ready { projects(&kinds, places, logger, stopped) } else { BTreeMap::new() };
        let image = |source: &Source| match kinds.get(source)?.as_ref()? {
            _ if !images_ready => None,
            Kind::Image(path) => Some(Picture::Image(path.clone())),
            Kind::Project { folder, preview } => {
                Some(Picture::Dynamic { project: folder.clone(), preview: preview.clone() })
            }
            Kind::Video(_) => Some(Picture::Dynamic { project: projects.get(source)?.clone(), preview: None }),
        };
        let mut plan = Self::default();
        match outputs {
            Outputs::Unknown => {}
            Outputs::Known(names) => {
                for (name, source) in choice.per_output(names) {
                    if let Some(path) = image(&source) {
                        plan.images.outputs.insert(name.clone(), path);
                    }
                    if let Some(project) = projects.get(&source) {
                        plan.screens.insert(name, project.clone());
                    }
                }
            }
            Outputs::Unfollowed => {
                plan.images.path = choice.path.as_ref().and_then(image);
                plan.images.outputs =
                    choice.outputs.iter().filter_map(|(name, source)| Some((name.clone(), image(source)?))).collect();
                plan.screens = choice
                    .outputs
                    .iter()
                    .filter_map(|(name, source)| Some((name.clone(), projects.get(source)?.clone())))
                    .collect();
                if let Some(every) = choice.path.as_ref().filter(|every| projects.contains_key(every)) {
                    let why = "novad doesn't follow this compositor's outputs, so set it for each output by name";
                    logger.warn(LOG_SOURCE, &format!("can't show {every} on every output: {why}"));
                }
            }
        }
        plan
    }
}
