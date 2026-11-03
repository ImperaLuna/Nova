//! The launcher's providers (`launcher.providers`): each one answers a search
//! with items and runs the one picked. A feature offers one by slotting a view
//! into `launcher.providers` in its manifest and naming the running service
//! that answers for that view in `find`; the host asks the ready ones in their
//! slot order. A provider with a trigger (like `=`) answers only queries that
//! start with it, and then it alone (with any others sharing it) does. One
//! with a mode of its own (files) answers only searches made in that mode,
//! which the launcher names, and never shows among the others.

use super::Launcher;
use crate::compositor::Compositor;
use crate::files::Files;
use crate::logger::Logger;
use crate::modules::{Has, Module, Running};
use crate::tray::Tray;
use nova_protocol::{
    FeatureState, FeatureStatus, LAUNCHER_SEARCH_MAX_CHARS, LauncherItem, PROVIDER_APPS, PROVIDER_FILES,
};
use std::sync::Arc;

/// The host providers slot into.
pub const HOST: &str = "launcher.providers";
const LOG_SOURCE: &str = "launcher";

/// What answers a launcher search and runs what it found.
pub trait Provider: Send + Sync {
    /// A prefix that sends a query to this provider alone, without it.
    /// `None` answers every query.
    fn trigger(&self) -> Option<&str> {
        None
    }

    /// It answers only searches made in its own mode (`LauncherSearch`'s
    /// `mode` naming it), given the whole query, and no others.
    fn own_mode(&self) -> bool {
        false
    }

    /// What it finds for `query` (never blank, and without the trigger),
    /// best first. The host fills in each item's `provider`.
    fn search(&self, query: &str) -> Result<Vec<LauncherItem>, String>;

    /// Runs the item `id` it found. May take a while: the server calls it
    /// off the connection's thread.
    fn run(&self, id: &str, how: &Run) -> Result<(), String>;
}

/// How and where to run an item, and what running it may use.
pub struct Run<'a> {
    /// The screen to open on; the focused one when unset.
    pub output: Option<&'a str>,
    /// Shift+Enter, the item's other way to run: an app in a new window
    /// even when one is open, a file shown in its folder.
    pub new_window: bool,
    pub compositor: Option<&'a Compositor>,
    pub tray: Option<&'a Tray>,
    pub logger: &'a Logger,
    /// Says the item is underway when what's left only waits on it (an app
    /// asked to show or to start, its window still to focus), so the
    /// launcher closes then instead of when `run` returns.
    pub underway: &'a dyn Fn(),
}

/// The running module that answers a provider view, while its feature runs.
type Finds = fn(&Running) -> Option<Arc<dyn Provider>>;

/// Every provider view and the module that answers it: the one line a new
/// provider adds outside its folder.
const PROVIDERS: &[(&str, Finds)] = &[(PROVIDER_APPS, running::<Launcher>), (PROVIDER_FILES, running::<Files>)];

/// The module `M`, while it runs, as a provider.
fn running<M: Module + Provider>(modules: &Running) -> Option<Arc<dyn Provider>>
where
    Running: Has<M>,
{
    modules.get::<M>().map(|module| module as Arc<dyn Provider>)
}

/// The running module that answers the provider view `view`.
pub fn find(view: &str, modules: &Running) -> Option<Arc<dyn Provider>> {
    PROVIDERS.iter().find(|(known, _)| *known == view).and_then(|(_, finds)| finds(modules))
}

/// The ready features' providers, in their slot order.
pub struct Host {
    providers: Vec<(String, Arc<dyn Provider>)>,
}

impl Host {
    /// The providers the ready features in `features` slot in, sorted by
    /// `order`, then feature id. `find` gives each view's provider; a view
    /// without one (its service stopped meanwhile) is left out.
    pub fn new(features: &[FeatureState], find: impl Fn(&str) -> Option<Arc<dyn Provider>>) -> Self {
        let mut slotted: Vec<(i64, &str, &str)> = features
            .iter()
            .filter(|feature| feature.status == FeatureStatus::Ready)
            .flat_map(|feature| {
                feature
                    .slots
                    .iter()
                    .filter(|slot| slot.host == HOST)
                    .map(|slot| (slot.order, feature.id.as_str(), slot.view.as_str()))
            })
            .collect();
        slotted.sort();
        let providers = slotted
            .into_iter()
            .filter_map(|(_, _, view)| find(view).map(|provider| (view.to_string(), provider)))
            .collect();
        Self { providers }
    }

    /// What the providers find for `query` in the launcher's `mode` (see
    /// `Command::LauncherSearch`). One that fails is logged and left out, so
    /// the others still show.
    pub fn search(&self, query: &str, mode: Option<&str>, logger: &Logger) -> Result<Vec<LauncherItem>, String> {
        if query.chars().count() > LAUNCHER_SEARCH_MAX_CHARS {
            return Err(format!("searches are at most {LAUNCHER_SEARCH_MAX_CHARS} characters"));
        }
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        if let Some(mode) = mode {
            let provider = self.provider(mode)?;
            return Ok(Self::ask(vec![(mode, provider.as_ref(), query)], logger));
        }
        // Providers with a mode of their own answer only in it.
        let mixed = || self.providers.iter().filter(|(_, provider)| !provider.own_mode());
        let triggered: Vec<(&str, &dyn Provider, &str)> = mixed()
            .filter_map(|(view, provider)| {
                let rest = query.strip_prefix(provider.trigger().filter(|trigger| !trigger.is_empty())?)?;
                Some((view.as_str(), provider.as_ref(), rest))
            })
            .collect();
        let asked = if triggered.is_empty() {
            mixed()
                .filter(|(_, provider)| provider.trigger().is_none())
                .map(|(view, provider)| (view.as_str(), provider.as_ref(), query))
                .collect()
        } else {
            triggered
        };
        Ok(Self::ask(asked, logger))
    }

    /// What each of `asked` (a provider's view, the provider, its query)
    /// finds, in order.
    fn ask(asked: Vec<(&str, &dyn Provider, &str)>, logger: &Logger) -> Vec<LauncherItem> {
        let mut found = Vec::new();
        for (view, provider, query) in asked {
            if query.trim().is_empty() {
                continue;
            }
            match provider.search(query) {
                Ok(items) => {
                    found.extend(items.into_iter().map(|item| LauncherItem { provider: view.to_string(), ..item }))
                }
                Err(e) => logger.warn(LOG_SOURCE, &format!("provider {view} can't search: {e}")),
            }
        }
        found
    }

    /// The provider `view`, or why there's none.
    pub fn provider(&self, view: &str) -> Result<Arc<dyn Provider>, String> {
        self.providers
            .iter()
            .find(|(id, _)| id == view)
            .map(|(_, provider)| Arc::clone(provider))
            .ok_or_else(|| format!("no launcher provider {view:?}"))
    }
}

/// Installed apps: the launcher's own provider.
impl Provider for Launcher {
    fn search(&self, query: &str) -> Result<Vec<LauncherItem>, String> {
        Ok(self.search_apps(query))
    }

    /// Focuses a window the app already has (unless `new_window`, or it's
    /// in the tray: its window shows), or else starts it.
    fn run(&self, id: &str, how: &Run) -> Result<(), String> {
        self.launch(id, |ids| {
            // A new window is asked for: no open one, nor the tray's.
            let (ids, tray) = if how.new_window { (&[][..], None) } else { (ids, how.tray) };
            let start = || self.start_app(id);
            super::focus_open(id, ids, how.compositor, tray, how.output, start, how.underway, how.logger)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::Hub;
    use nova_protocol::{LogLevel, Slot, TextRun};
    use nova_test_support::TempDir;
    use std::sync::Mutex;

    /// A logger writing into `dir`.
    fn logger(dir: &TempDir) -> Logger {
        Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::new(Hub::default())).unwrap()
    }

    /// A provider that finds `query` back as one item and remembers runs.
    struct Echo {
        trigger: Option<&'static str>,
        own_mode: bool,
        ran: Mutex<Vec<String>>,
    }

    impl Provider for Echo {
        fn trigger(&self) -> Option<&str> {
            self.trigger
        }
        fn own_mode(&self) -> bool {
            self.own_mode
        }
        fn search(&self, query: &str) -> Result<Vec<LauncherItem>, String> {
            if query == "fail" {
                return Err("broken".into());
            }
            Ok(vec![LauncherItem {
                provider: String::new(),
                id: query.into(),
                name: vec![TextRun { text: query.into(), matched: false }],
                description: None,
                icon: None,
                file: None,
            }])
        }
        fn run(&self, id: &str, _: &Run) -> Result<(), String> {
            self.ran.lock().unwrap().push(id.into());
            Ok(())
        }
    }

    /// A feature `id` in `status`, filling `host` with the view `id` at `order`.
    fn feature(id: &str, status: FeatureStatus, host: &str, order: i64) -> FeatureState {
        let slot = Slot { host: host.into(), view: id.into(), order };
        FeatureState { id: id.into(), status, missing: Vec::new(), slots: vec![slot] }
    }

    fn provider(id: &str, order: i64) -> FeatureState {
        feature(id, FeatureStatus::Ready, HOST, order)
    }

    fn echo(trigger: Option<&'static str>) -> Arc<Echo> {
        Arc::new(Echo { trigger, own_mode: false, ran: Mutex::new(Vec::new()) })
    }

    fn found(items: &[LauncherItem]) -> Vec<(&str, &str)> {
        items.iter().map(|item| (item.provider.as_str(), item.id.as_str())).collect()
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
        let host = Host::new(&features, |_| Some(echo(None) as Arc<dyn Provider>));
        let dir = TempDir::new("providers-order");
        let logger = logger(&dir);
        let items = host.search("fi", None, &logger).unwrap();
        assert_eq!(found(&items), [("alpha", "fi"), ("zeta", "fi"), ("calc", "fi")]);
        assert!(host.provider("off").is_err());
        assert!(host.provider("bar").is_err());
    }

    #[test]
    fn a_trigger_sends_the_query_to_its_providers_alone() {
        let features = [provider("apps", 10), provider("calc", 20)];
        let calc = echo(Some("="));
        let host = Host::new(&features, |view| {
            Some(if view == "calc" { Arc::clone(&calc) as Arc<dyn Provider> } else { echo(None) })
        });
        let dir = TempDir::new("providers-trigger");
        let logger = logger(&dir);
        assert_eq!(found(&host.search("=1+2", None, &logger).unwrap()), [("calc", "1+2")]);
        assert_eq!(found(&host.search("1+2", None, &logger).unwrap()), [("apps", "1+2")]);
        assert!(host.search("=", None, &logger).unwrap().is_empty());
        assert!(host.search("  ", None, &logger).unwrap().is_empty());
        assert!(host.search(&"x".repeat(LAUNCHER_SEARCH_MAX_CHARS + 1), None, &logger).is_err());
    }

    #[test]
    fn a_provider_with_its_own_mode_answers_only_in_it_and_gets_the_whole_query() {
        let features = [provider("apps", 10), provider("files", 20)];
        let files = Arc::new(Echo { trigger: None, own_mode: true, ran: Mutex::new(Vec::new()) });
        let host = Host::new(&features, |view| {
            Some(if view == "files" { Arc::clone(&files) as Arc<dyn Provider> } else { echo(None) })
        });
        let dir = TempDir::new("providers-mode");
        let logger = logger(&dir);
        assert_eq!(found(&host.search("~/Doc", None, &logger).unwrap()), [("apps", "~/Doc")]);
        assert_eq!(found(&host.search("~/Doc", Some("files"), &logger).unwrap()), [("files", "~/Doc")]);
        assert_eq!(found(&host.search("fi", Some("apps"), &logger).unwrap()), [("apps", "fi")]);
        assert!(host.search(" ", Some("files"), &logger).unwrap().is_empty());
        assert!(host.search("fi", Some("calc"), &logger).is_err());
    }

    #[test]
    fn a_failing_provider_leaves_the_others_and_runs_reach_the_one_named() {
        let features = [provider("apps", 10), provider("calc", 20)];
        let calc = echo(None);
        let host = Host::new(&features, |view| {
            Some(if view == "calc" { Arc::clone(&calc) as Arc<dyn Provider> } else { echo(None) })
        });
        let dir = TempDir::new("providers-fail");
        let logger = logger(&dir);
        assert_eq!(found(&host.search("fail", None, &logger).unwrap()), []);
        let how =
            Run { output: None, new_window: false, compositor: None, tray: None, logger: &logger, underway: &|| {} };
        host.provider("calc").unwrap().run("1+2", &how).unwrap();
        assert_eq!(*calc.ran.lock().unwrap(), ["1+2"]);
        assert!(host.provider("nothing").is_err());
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
