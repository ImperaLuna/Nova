//! Runs the launcher feature (`nova-launcher`, in `features/launcher`) in
//! novad: its module, the answers to its commands, and its ports. Wiring
//! only: it adapts the feature's core to novad's `Module` trait and
//! `Daemon`, fills the core's ports with today's modules (the tray's apps,
//! and the module answering each provider view, [`PROVIDERS`]), and all of
//! the feature's work and state stay in the core.

use crate::Daemon;
use crate::modules::{Has, Module, Running};
use crate::nova_files::NovaFiles;
use crate::{lock, tray::Tray};
use nova_launcher::providers::{HOST, Host, Provider, Run, Slotted};
use nova_launcher_api::{Command, PROVIDER_APPS};
use nova_nova_files_api::PROVIDER_FILES;
use nova_protocol::{FeatureState, FeatureStatus, Outcome};
use nova_socket::{Answer, Caller, done, value};
use std::sync::{Arc, mpsc};

pub use nova_launcher::{FEATURE_ID, Paths, SHOWN_WINDOW_WAIT, STARTED_WINDOW_WAIT, autostart, focus_shown};

/// The feature's core, as novad's module.
pub struct Launcher(Arc<nova_launcher::Launcher>);

impl std::ops::Deref for Launcher {
    type Target = nova_launcher::Launcher;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The answer to one of the feature's commands, the route
/// `requests::router` gives them.
pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    Answer::Now(match command {
        // A provider's own mode (files) waits on another app (Nova
        // Files), so it's answered off the connection's thread: a slow
        // answer must not hold up other requests. The mixed providers answer from memory.
        Command::LauncherSearch { query, mode } => {
            let providers = match providers(daemon) {
                Ok(providers) => providers,
                Err(e) => return Answer::Now(Outcome::Error(e)),
            };
            if mode.is_none() {
                return Answer::Now(value(providers.search(query, None, &daemon.logger)));
            }
            let (query, mode, logger) = (query.clone(), mode.clone(), Arc::clone(&daemon.logger));
            return Answer::later(move || value(providers.search(&query, mode.as_deref(), &logger)));
        }
        Command::AppIcon { icon_name, program, name } => {
            let apps = Arc::clone(&lock(&daemon.settings).context.apps);
            value(apps.app_icon(&daemon.sources.icons, icon_name, program, name))
        }
        Command::LauncherRun { provider, id, output, new_window } => {
            return run(daemon, provider, id, output.clone(), *new_window);
        }
        Command::PinApp { id, pinned } => {
            done(daemon.module::<Launcher>().and_then(|launcher| launcher.pin(id, *pinned)))
        }
    })
}

/// The ready features' launcher providers, while the launcher runs.
fn providers(daemon: &Daemon) -> Result<Host, String> {
    daemon.module::<Launcher>()?;
    let settings = lock(&daemon.settings);
    Ok(Host::new(slotted(&settings.features), |view| find(view, &settings.modules)))
}

/// The provider views the ready features in `features` slot into the
/// launcher's host.
fn slotted(features: &[FeatureState]) -> impl Iterator<Item = Slotted<'_>> {
    features.iter().filter(|feature| feature.status == FeatureStatus::Ready).flat_map(|feature| {
        feature.slots.iter().filter(|slot| slot.host == HOST).map(|slot| Slotted {
            order: slot.order,
            feature: &feature.id,
            view: &slot.view,
        })
    })
}

/// Runs what the launcher's provider `provider` found as `id`, like an
/// app it focuses or starts (see `nova_launcher::providers`). Answers once it
/// is underway: the launcher waits for the answer to close, and an app
/// in the tray may take seconds to show its window. A failure after
/// that is only logged.
pub fn run(daemon: &Daemon, provider: &str, id: &str, output: Option<String>, new_window: bool) -> Answer {
    let provider = match providers(daemon).and_then(|providers| providers.provider(provider)) {
        Ok(provider) => provider,
        Err(e) => return Answer::Now(Outcome::Error(e)),
    };
    let compositor = daemon.compositor.clone();
    let tray = daemon.module::<Tray>().ok();
    let logger = Arc::clone(&daemon.logger);
    let id = id.to_owned();
    Answer::later(move || {
        let (answer, answered) = mpsc::channel();
        std::thread::spawn(move || {
            let early = answer.clone();
            let underway = move || {
                let _ = early.send(Ok(()));
            };
            let how = Run {
                output: output.as_deref(),
                new_window,
                compositor: compositor.as_deref(),
                tray: tray.as_deref().map(|tray| tray as &dyn nova_launcher::TrayApps),
                logger: &logger,
                underway: &underway,
            };
            // Unheard once it was underway; starting the app logs its
            // own failures.
            let _ = answer.send(provider.run(&id, &how));
        });
        done(answered.recv().unwrap_or_else(|_| Err("the launch stopped without an answer".into())))
    })
}

/// The launcher's port for the apps in the tray, filled by the tray's
/// module.
impl nova_launcher::TrayApps for Tray {
    fn in_tray(&self, app_id: &str) -> bool {
        self.has_app(app_id)
    }

    fn activate_in_tray(&self, app_id: &str) -> Result<bool, String> {
        self.activate_app(app_id)
    }
}

/// The running module that answers a provider view, while its feature runs.
type Finds = fn(&Running) -> Option<Arc<dyn Provider>>;

/// Every provider view and the module that answers it: the one line a new
/// provider adds outside its folder.
const PROVIDERS: &[(&str, Finds)] = &[(PROVIDER_APPS, apps), (PROVIDER_FILES, running::<NovaFiles>)];

/// The launcher's own provider, the installed apps, while it runs.
fn apps(modules: &Running) -> Option<Arc<dyn Provider>> {
    modules.get::<Launcher>().map(|launcher| Arc::clone(&launcher.0) as Arc<dyn Provider>)
}

/// The module `M`, while it runs, as a provider.
fn running<M: Module + Provider>(modules: &Running) -> Option<Arc<dyn Provider>>
where
    Running: Has<M>,
{
    modules.get::<M>().map(|module| module as Arc<dyn Provider>)
}

/// The running module that answers the provider view `view`.
fn find(view: &str, modules: &Running) -> Option<Arc<dyn Provider>> {
    PROVIDERS.iter().find(|(known, _)| *known == view).and_then(|(_, finds)| finds(modules))
}

/// It has no options: it only starts and stops with its feature.
impl crate::modules::Module for Launcher {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const NEEDS_APPS: bool = true;
    const TOPIC: Option<nova_protocol::Topic> = Some(nova_launcher_api::TOPIC);
    type Sources = Paths;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> Paths {
        Paths::new(&env.args.state_dir, env.path_var)
    }

    fn config(_: &crate::config::Config) {}

    fn start(ctx: &crate::modules::Context, paths: &Paths, (): &(), _: &[nova_protocol::FeatureState]) -> Arc<Self> {
        Arc::new(Self(nova_launcher::Launcher::start(
            paths.clone(),
            Arc::clone(&ctx.apps),
            &ctx.path_var,
            Arc::clone(&ctx.hub),
            Arc::clone(&ctx.logger),
        )))
    }

    fn send_state(&self, client: crate::hub::ClientId) {
        self.0.send_state(client);
    }

    fn stop(&self) {
        self.0.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_launcher_api::{LauncherItem, TextRun};
    use nova_protocol::{LogLevel, Slot};
    use nova_test_support::TempDir;

    /// A provider that finds `query` back as one item.
    struct Echo;

    impl Provider for Echo {
        fn search(&self, query: &str) -> Result<Vec<LauncherItem>, String> {
            Ok(vec![LauncherItem {
                provider: String::new(),
                id: query.into(),
                name: vec![TextRun { text: query.into(), matched: false }],
                description: None,
                icon: None,
                file: None,
            }])
        }
        fn run(&self, _: &str, _: &Run) -> Result<(), String> {
            Ok(())
        }
    }

    /// A feature `id` in `status`, filling `host` with the view `id` at `order`.
    fn feature(id: &str, status: FeatureStatus, host: &str, order: i64) -> FeatureState {
        let slot = Slot { host: host.into(), view: id.into(), order };
        FeatureState { id: id.into(), status, missing: Vec::new(), slots: vec![slot], mode: None }
    }

    fn provider(id: &str, order: i64) -> FeatureState {
        feature(id, FeatureStatus::Ready, HOST, order)
    }

    #[test]
    fn providers_answer_in_slot_order_and_only_while_their_feature_is_ready() {
        let features = [
            provider("zeta", 10),
            provider("calc", 20),
            provider("alpha", 10),
            feature("off", FeatureStatus::Off, HOST, 0),
            feature("bar", FeatureStatus::Ready, "bar.left", 0),
        ];
        let host = Host::new(slotted(&features), |_| Some(Arc::new(Echo) as Arc<dyn Provider>));
        let dir = TempDir::new("providers-order");
        let logger =
            crate::logger::Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::new(crate::Hub::default()))
                .unwrap();
        let items = host.search("fi", None, &logger).unwrap();
        let found: Vec<(&str, &str)> = items.iter().map(|item| (item.provider.as_str(), item.id.as_str())).collect();
        assert_eq!(found, [("alpha", "fi"), ("zeta", "fi"), ("calc", "fi")]);
        assert!(host.provider("off").is_err());
        assert!(host.provider("bar").is_err());
    }

    /// Every provider view a manifest in the repo slots in has a provider.
    #[test]
    fn every_provider_in_the_repo_is_known() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        let views: Vec<String> = nova_features::load_all(&root)
            .unwrap()
            .iter()
            .flat_map(|manifest| {
                manifest.slots.iter().filter(|slot| slot.host == HOST).map(|slot| slot.view.clone()).collect::<Vec<_>>()
            })
            .collect();
        assert!(views.contains(&PROVIDER_APPS.to_string()));
        let host = nova_features::host(HOST).expect("launcher.providers is in nova_features::HOSTS");
        assert_eq!(host.views, nova_features::Views::Daemon);
        for view in views {
            assert!(
                PROVIDERS.iter().any(|(known, _)| *known == view),
                "no provider for the launcher view {view:?}; add it to PROVIDERS"
            );
        }
    }
}
