//! Runs the Nova Files feature (`nova-nova-files`, in `features/nova-files`)
//! in novad: its module, and its place in the launcher as the provider of
//! the Files mode (`nova-nova-files-api`'s `PROVIDER_FILES`). Wiring only:
//! it adapts the feature's core to novad's `Module` trait and to the
//! launcher's `Provider` port, and all of the feature's work and state stay
//! in the core.

use nova_launcher::providers::{Provider, Run};
use nova_launcher_api::LauncherItem;
use nova_nova_files::ID;
pub use nova_nova_files::Sources;
use nova_protocol::FeatureState;
use std::path::Path;
use std::sync::Arc;

/// The feature's core, as novad's module.
pub struct NovaFiles(nova_nova_files::NovaFiles);

/// The launcher's Files mode: a mode of its own, answered by the core.
impl Provider for NovaFiles {
    fn own_mode(&self) -> bool {
        true
    }

    fn search(&self, query: &str) -> Result<Vec<LauncherItem>, String> {
        self.0.items(query)
    }

    fn run(&self, id: &str, how: &Run) -> Result<(), String> {
        self.0.run_item(id, how.new_window)
    }
}

impl crate::modules::Module for NovaFiles {
    type App = crate::modules::Novad;
    const ID: &'static str = ID;
    type Sources = Sources;
    type Config = ();

    fn sources(env: &crate::modules::Env) -> Sources {
        let home = env.home.clone().filter(|home| home != Path::new("/"));
        Sources { bus: nova_session_bus::Bus::Session, data_dirs: nova_paths::data_dirs(), home }
    }

    fn config(_: &crate::config::Config) {}

    fn start(ctx: &crate::modules::Context, sources: &Sources, _: &(), features: &[FeatureState]) -> Arc<Self> {
        Arc::new(Self(nova_nova_files::NovaFiles::start(sources, own_windows(features), Arc::clone(&ctx.logger))))
    }

    /// A mode change that adds or drops Nova Files' windows restarts the
    /// module, which reads the mode when it starts.
    fn reconfigure(&self, _: &(), _: &(), features: &[FeatureState]) -> crate::modules::Reconfigure {
        if own_windows(features) == self.0.own_windows() {
            crate::modules::Reconfigure::InPlace
        } else {
            crate::modules::Reconfigure::Restart
        }
    }

    fn stop(&self) {
        self.0.stop();
    }
}

/// Whether Nova Files opens its own windows in the mode `features` give
/// it.
fn own_windows(features: &[FeatureState]) -> bool {
    let mode = features.iter().find(|feature| feature.id == ID).and_then(|feature| feature.mode.as_deref());
    nova_nova_files::windows_in(mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{Module, Reconfigure};
    use nova_nova_files::LAUNCHER_MODE;
    use nova_protocol::FeatureStatus;
    use nova_test_support::TempDir;

    fn state(mode: Option<&str>) -> FeatureState {
        FeatureState {
            id: ID.into(),
            status: FeatureStatus::Ready,
            missing: vec![],
            slots: vec![],
            mode: mode.map(str::to_string),
        }
    }

    /// Switching between a mode with windows and the launcher mode restarts
    /// the module, so it routes folders the new way; other changes keep it.
    #[test]
    fn a_change_to_or_from_the_launcher_mode_restarts_the_module() {
        let dir = TempDir::new("nova-files-reconfigure");
        let logger = nova_log::Logger::open(
            &dir.join("logs"),
            nova_protocol::LogLevel::Debug,
            false,
            Arc::new(nova_hub::Hub::default()),
        )
        .unwrap();
        // No bus there: it can't reach Nova Files, which a mode change doesn't need.
        let sources = Sources {
            bus: nova_session_bus::Bus::Address(format!("unix:path={}", dir.join("no-bus").display())),
            data_dirs: vec![],
            home: None,
        };
        let with_windows = NovaFiles(nova_nova_files::NovaFiles::start(&sources, true, Arc::new(logger)));
        assert!(with_windows.own_mode());
        assert_eq!(with_windows.reconfigure(&(), &(), &[state(Some("full"))]), Reconfigure::InPlace);
        assert_eq!(with_windows.reconfigure(&(), &(), &[state(Some(LAUNCHER_MODE))]), Reconfigure::Restart);
        with_windows.stop();
    }

    #[test]
    fn only_the_launcher_mode_goes_without_nova_files_windows() {
        assert!(own_windows(&[state(Some("basic"))]));
        assert!(own_windows(&[state(Some("full"))]));
        assert!(own_windows(&[state(None)]));
        assert!(!own_windows(&[state(Some(LAUNCHER_MODE))]));
    }
}
