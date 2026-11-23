use super::{Launcher, TrayApps};
use nova_compositor::Compositor;
use nova_launcher_api::{LAUNCHER_SEARCH_MAX_CHARS, LauncherItem};
use nova_log::Logger;
use std::sync::Arc;

pub const HOST: &str = "launcher.providers";
const LOG_SOURCE: &str = "launcher";

pub trait Provider: Send + Sync {
    fn trigger(&self) -> Option<&str> {
        None
    }

    fn own_mode(&self) -> bool {
        false
    }

    fn search(&self, query: &str) -> Result<Vec<LauncherItem>, String>;

    fn run(&self, id: &str, how: &Run) -> Result<(), String>;
}

pub struct Run<'a> {
    pub output: Option<&'a str>,
    pub new_window: bool,
    pub compositor: Option<&'a Compositor>,
    pub tray: Option<&'a dyn TrayApps>,
    pub logger: &'a Logger,
    pub underway: &'a dyn Fn(),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slotted<'a> {
    pub order: i64,
    pub feature: &'a str,
    pub view: &'a str,
}

pub struct Host {
    providers: Vec<(String, Arc<dyn Provider>)>,
}

impl Host {
    pub fn new<'a>(
        slotted: impl IntoIterator<Item = Slotted<'a>>,
        find: impl Fn(&str) -> Option<Arc<dyn Provider>>,
    ) -> Self {
        let mut slotted: Vec<(i64, &str, &str)> =
            slotted.into_iter().map(|slot| (slot.order, slot.feature, slot.view)).collect();
        slotted.sort();
        let providers = slotted
            .into_iter()
            .filter_map(|(_, _, view)| find(view).map(|provider| (view.to_string(), provider)))
            .collect();
        Self { providers }
    }

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

    pub fn provider(&self, view: &str) -> Result<Arc<dyn Provider>, String> {
        self.providers
            .iter()
            .find(|(id, _)| id == view)
            .map(|(_, provider)| Arc::clone(provider))
            .ok_or_else(|| format!("no launcher provider {view:?}"))
    }
}

impl Provider for Launcher {
    fn search(&self, query: &str) -> Result<Vec<LauncherItem>, String> {
        Ok(self.search_apps(query))
    }

    fn run(&self, id: &str, how: &Run) -> Result<(), String> {
        self.launch(id, |ids| {
            let (ids, tray) = if how.new_window { (&[][..], None) } else { (ids, how.tray) };
            let start = || self.start_app(id);
            super::focus_open(id, ids, &Run { tray, ..*how }, start)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_carrier::log::LogLevel;
    use nova_hub::Hub;
    use nova_launcher_api::TextRun;
    use nova_test_support::TempDir;
    use std::sync::Mutex;

    fn logger(dir: &TempDir) -> Logger {
        Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::new(Hub::default())).unwrap()
    }

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

    fn provider(id: &'static str, order: i64) -> Slotted<'static> {
        Slotted { order, feature: id, view: id }
    }

    fn echo(trigger: Option<&'static str>) -> Arc<Echo> {
        Arc::new(Echo { trigger, own_mode: false, ran: Mutex::new(Vec::new()) })
    }

    fn found(items: &[LauncherItem]) -> Vec<(&str, &str)> {
        items.iter().map(|item| (item.provider.as_str(), item.id.as_str())).collect()
    }

    #[test]
    fn providers_answer_in_slot_order() {
        let features = [provider("zeta", 10), provider("calc", 20), provider("alpha", 10)];
        let host = Host::new(features, |_| Some(echo(None) as Arc<dyn Provider>));
        let dir = TempDir::new("providers-order");
        let logger = logger(&dir);
        let items = host.search("fi", None, &logger).unwrap();
        assert_eq!(found(&items), [("alpha", "fi"), ("zeta", "fi"), ("calc", "fi")]);
    }

    #[test]
    fn a_trigger_sends_the_query_to_its_providers_alone() {
        let features = [provider("apps", 10), provider("calc", 20)];
        let calc = echo(Some("="));
        let host = Host::new(features, |view| {
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
        let host = Host::new(features, |view| {
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
        let host = Host::new(features, |view| {
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
}
