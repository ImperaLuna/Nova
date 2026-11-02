//! novad's modules: every feature's daemon side behind one [`Module`] trait,
//! listed once in the [`modules!`] registry below and run by one loop
//! ([`Running::apply`]) on every settings change, so a feature's daemon
//! side lives in its own folder plus one registry line.
//!
//! Two kinds: most run while their feature is ready, started when it turns
//! ready, stopped when it doesn't and restarted when their options change
//! unless they apply them in place. The few that hold state across their
//! feature turning off and on (a recording to stop, the lock screen, the
//! region picker, the wallpaper) are kept for novad's whole life and only
//! told what the settings are now, right after they start and on every
//! change. They never restart, so the daemon holds them ([`Kept`]) and
//! reaches them without the settings lock.
//!
//! The [`Context`] is what modules share: the hub, the logger, the
//! compositor, and the app index, which follows the apps while any module
//! that names them runs ([`Module::NEEDS_APPS`]).

use crate::apps::{self, AppIndex};
use crate::compositor::Compositor;
use crate::config::Config;
use crate::hub::{ClientId, Hub};
use crate::icons::Icons;
use crate::logger::Logger;
use crate::{
    ai_usage, bluetooth, features, launcher, media, network, notifications, peripherals, recording, resources,
    screenshot, session_lock, tray, wallpaper, x11_tray,
};
use nova_protocol::{FeatureState, Topic};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const LOG_SOURCE: &str = "features";

/// The daemon side of a feature.
pub trait Module: Send + Sync + Sized + 'static {
    /// Its feature's id in `features/`.
    const ID: &'static str;
    /// It names apps, so the app index runs while it does.
    const NEEDS_APPS: bool = false;
    /// The topic whose current state a new subscriber gets from it.
    const TOPIC: Option<Topic> = None;
    /// Where it reads and keeps things, fixed for novad's life.
    type Sources: Clone;
    /// Its options in `config.toml`.
    type Config: Clone + PartialEq;

    /// Its sources on this machine.
    fn sources(env: &Env) -> Self::Sources;
    /// Its options, out of the whole config.
    fn config(config: &Config) -> Self::Config;
    /// Starts it. Returns once it's ready to answer.
    fn start(ctx: &Context, sources: &Self::Sources, config: &Self::Config, features: &[FeatureState]) -> Arc<Self>;
    /// The settings changed while it runs, from options `before` to
    /// `config`: applies them in place, or asks to be stopped and started
    /// again (the default when its options changed). A kept module is told
    /// right after it starts too, and always applies them in place.
    fn reconfigure(&self, before: &Self::Config, config: &Self::Config, features: &[FeatureState]) -> Reconfigure {
        let _ = features;
        if before == config { Reconfigure::InPlace } else { Reconfigure::Restart }
    }
    /// After every change, with the modules running now, for one that uses
    /// another's service.
    fn linked(&self, running: &Running) {
        let _ = running;
    }
    /// Sends a new subscriber to [`Module::TOPIC`] the current state.
    fn send_state(&self, client: ClientId) {
        let _ = client;
    }
    /// The client `client` hung up.
    fn client_gone(&self, client: ClientId) {
        let _ = client;
    }
    /// Stops it: it publishes nothing more.
    fn stop(&self);
}

/// What a running module does with changed settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconfigure {
    InPlace,
    Restart,
}

/// What modules find their sources from, on this machine.
pub struct Env<'a> {
    pub args: &'a crate::Args,
    /// novad's runtime folder, its socket's.
    pub runtime_dir: &'a Path,
    pub path_var: &'a str,
    pub icons: &'a Arc<Icons>,
    pub home: Option<PathBuf>,
}

/// What modules share.
pub struct Context {
    pub hub: Arc<Hub>,
    pub logger: Arc<Logger>,
    /// Where programs are looked for.
    pub path_var: String,
    /// Where app icons are found.
    pub icons: Arc<Icons>,
    /// Following the compositor, when it's one novad knows.
    pub compositor: Option<Arc<Compositor>>,
    /// The installed apps, known while a module that names them runs.
    pub apps: Arc<AppIndex>,
}

/// A running module and the options it runs with.
struct Slot<M: Module> {
    module: Arc<M>,
    config: M::Config,
}

/// A module of the registry, reached by its type.
pub trait Has<M: Module> {
    fn module(&self) -> Option<&Arc<M>>;
}

impl Running {
    /// The module `M`, while it runs.
    pub fn get<M: Module>(&self) -> Option<Arc<M>>
    where
        Self: Has<M>,
    {
        Has::<M>::module(self).cloned()
    }
}

/// The registry: modules that run while their feature is ready, then the
/// ones kept for novad's life. Within each, the order modules start in.
macro_rules! modules {
    (
        while_ready: [$($field:ident: $module:ty),* $(,)?],
        kept: [$($kept:ident: $kept_module:ty),* $(,)?] $(,)?
    ) => {
        /// Where each module reads and keeps things.
        #[derive(Clone)]
        pub struct Sources {
            pub apps: apps::Sources,
            $(pub $field: <$module as Module>::Sources,)*
            $(pub $kept: <$kept_module as Module>::Sources,)*
        }

        impl Sources {
            pub fn system(env: &Env) -> Self {
                Self {
                    apps: apps::Sources::system(Arc::clone(env.icons)),
                    $($field: <$module>::sources(env),)*
                    $($kept: <$kept_module>::sources(env),)*
                }
            }
        }

        /// The modules running.
        pub struct Running {
            $($field: Option<Slot<$module>>,)*
            $($kept: Slot<$kept_module>,)*
        }

        impl Running {
            /// Every module's feature id.
            #[cfg(test)]
            pub const IDS: &[&str] = &[$(<$module>::ID,)* $(<$kept_module>::ID,)*];

            /// Starts the modules kept for novad's life, and those whose
            /// feature is ready.
            pub fn start(ctx: &Context, sources: &Sources, config: &Config, features: &[FeatureState]) -> Self {
                let mut running = Self {
                    $($field: None,)*
                    $($kept: start(ctx, &sources.$kept, config, features),)*
                };
                running.apply(ctx, sources, config, features);
                running
            }

            /// Starts, stops, restarts and reconfigures modules to match
            /// `config` and `features`, then links them up.
            pub fn apply(&mut self, ctx: &Context, sources: &Sources, config: &Config, features: &[FeatureState]) {
                let apps_needed = false $(|| (<$module>::NEEDS_APPS && features::is_ready(features, <$module>::ID)))*;
                // Followed before a module that names apps starts, and
                // forgotten once the last of them has stopped.
                if apps_needed {
                    ctx.apps.follow();
                }
                $(reconcile(&mut self.$field, ctx, &sources.$field, config, features);)*
                $(keep(&mut self.$kept, config, features);)*
                if !apps_needed {
                    ctx.apps.forget();
                }
                $(if let Some(slot) = &self.$field { Module::linked(&*slot.module, self); })*
                $(Module::linked(&*self.$kept.module, self);)*
            }

            /// Sends `client` the current state for `topic`.
            pub fn send_state(&self, topic: Topic, client: ClientId) {
                $(if <$module>::TOPIC == Some(topic) && let Some(slot) = &self.$field {
                    Module::send_state(&*slot.module, client);
                })*
                $(if <$kept_module>::TOPIC == Some(topic) {
                    Module::send_state(&*self.$kept.module, client);
                })*
            }

            /// Tells every module running that `client` hung up.
            pub fn client_gone(&self, client: ClientId) {
                $(if let Some(slot) = &self.$field { Module::client_gone(&*slot.module, client); })*
                $(Module::client_gone(&*self.$kept.module, client);)*
            }

            /// Stops every module, for a novad that ends without exiting the
            /// process ([`crate::InProcess`]).
            pub fn stop_all(&mut self) {
                $(if let Some(slot) = self.$field.take() { Module::stop(&*slot.module); })*
                $(Module::stop(&*self.$kept.module);)*
            }
        }

        $(impl Has<$module> for Running {
            fn module(&self) -> Option<&Arc<$module>> {
                self.$field.as_ref().map(|slot| &slot.module)
            }
        })*
        /// The modules kept for novad's life.
        #[derive(Clone)]
        pub struct Kept {
            $(pub $kept: Arc<$kept_module>,)*
        }

        impl Running {
            /// The modules kept for novad's life, for reaching them without
            /// the settings lock.
            pub fn kept(&self) -> Kept {
                Kept { $($kept: Arc::clone(&self.$kept.module),)* }
            }
        }

        $(impl Has<$kept_module> for Running {
            fn module(&self) -> Option<&Arc<$kept_module>> {
                Some(&self.$kept.module)
            }
        })*
    };
}

modules! {
    while_ready: [
        ai_usage: ai_usage::Usage,
        resources: resources::Resources,
        launcher: launcher::Launcher,
        tray: tray::Tray,
        notifications: notifications::Notifications,
        media: media::Media,
        peripherals: peripherals::Peripherals,
        network: network::Network,
        bluetooth: bluetooth::Bluetooth,
        x11_tray: x11_tray::X11Tray,
    ],
    kept: [
        session_lock: session_lock::SessionLock,
        screenshots: screenshot::Screenshots,
        recordings: recording::Recordings,
        wallpaper: wallpaper::Wallpaper,
    ],
}

/// Starts `M`, logged.
fn start<M: Module>(ctx: &Context, sources: &M::Sources, config: &Config, features: &[FeatureState]) -> Slot<M> {
    ctx.logger.info(LOG_SOURCE, &format!("starting {}", M::ID));
    let config = M::config(config);
    Slot { module: M::start(ctx, sources, &config, features), config }
}

/// `slot`'s module after a change: kept while its feature is ready and it
/// applies the change in place, else stopped; started while its feature is
/// ready and none is kept.
fn reconcile<M: Module>(
    slot: &mut Option<Slot<M>>,
    ctx: &Context,
    sources: &M::Sources,
    config: &Config,
    features: &[FeatureState],
) {
    let ready = features::is_ready(features, M::ID);
    if let Some(running) = slot.take() {
        let options = M::config(config);
        if ready && running.module.reconfigure(&running.config, &options, features) == Reconfigure::InPlace {
            *slot = Some(Slot { module: running.module, config: options });
            return;
        }
        running.module.stop();
        ctx.logger.info(LOG_SOURCE, &format!("stopped {}", M::ID));
    }
    if ready {
        *slot = Some(start(ctx, sources, config, features));
    }
}

/// A module kept for novad's life after a change: told the settings,
/// which it applies in place.
fn keep<M: Module>(slot: &mut Slot<M>, config: &Config, features: &[FeatureState]) {
    let options = M::config(config);
    let applied = slot.module.reconfigure(&slot.config, &options, features);
    debug_assert_eq!(applied, Reconfigure::InPlace, "{} is kept, so it can't restart", M::ID);
    slot.config = options;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each module is a feature whose daemon side names it.
    #[test]
    fn every_module_is_a_daemon_feature() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        let manifests = nova_features::load_all(&root).unwrap();
        for id in Running::IDS {
            let manifest = manifests.iter().find(|manifest| manifest.id == *id);
            assert_eq!(
                manifest.and_then(|manifest| manifest.daemon.as_deref()),
                Some(*id),
                "{id} has no feature whose `daemon` names it"
            );
        }
    }
}
