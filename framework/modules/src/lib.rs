//! The daemon side of every feature behind one [`Module`] trait, and the
//! [`modules!`] registry that runs them: the app lists its modules once, and
//! one loop (`Running::apply`, generated) starts, stops, restarts and
//! reconfigures them on every settings change, so a feature's daemon side
//! lives in its own folder plus one registry line.
//!
//! Two kinds: most run while their feature is ready, started when it turns
//! ready, stopped when it doesn't and restarted when their options change
//! unless they apply them in place. The few that hold state across their
//! feature turning off and on are kept for the app's whole life and only
//! told what the settings are now, right after they start and on every
//! change. They never restart, so the app holds them (`Kept`, generated) and
//! reaches them without its settings lock.
//!
//! Nothing here names a feature or the app's types: those come in through
//! [`App`], which the app implements once.

use nova_log::Logger;
use std::sync::Arc;

const LOG_SOURCE: &str = "features";

/// What the app gives its modules and runs them by.
pub trait App: Sized + 'static {
    /// What modules find their sources from, on this machine.
    type Env<'a>;
    /// The whole of the user's settings, each module's options in it.
    type Config;
    /// What modules share while they run.
    type Context;
    /// One feature's state: whether it's ready, its mode.
    type Feature;
    /// A topic a client subscribes to.
    type Topic: PartialEq + Copy;
    /// A connected client.
    type Client: Copy;
    /// The registry's running modules (`Running`, from [`modules!`]).
    type Running;
    /// Where the app index reads apps, for the modules that name them.
    type AppSources: Clone;

    fn app_sources(env: &Self::Env<'_>) -> Self::AppSources;
    /// Whether the feature `id` is ready.
    fn is_ready(features: &[Self::Feature], id: &str) -> bool;
    fn logger(ctx: &Self::Context) -> &Logger;
    /// The app index follows the apps from now on.
    fn follow_apps(ctx: &Self::Context);
    /// The app index stops following the apps.
    fn forget_apps(ctx: &Self::Context);
}

/// The settings of the app `A`.
pub type Config<A> = <A as App>::Config;
/// The feature states of the app `A`.
pub type Features<A> = [<A as App>::Feature];

/// The daemon side of a feature.
pub trait Module: Send + Sync + Sized + 'static {
    /// The app it runs in.
    type App: App;
    /// Its feature's id in `features/`.
    const ID: &'static str;
    /// It names apps, so the app index runs while it does.
    const NEEDS_APPS: bool = false;
    /// The topic whose current state a new subscriber gets from it.
    const TOPIC: Option<<Self::App as App>::Topic> = None;
    /// Where it reads and keeps things, fixed for the app's life.
    type Sources: Clone;
    /// Its options in the settings.
    type Config: Clone + PartialEq;

    /// Its sources on this machine.
    fn sources(env: &<Self::App as App>::Env<'_>) -> Self::Sources;
    /// Its options, out of the whole config.
    fn config(config: &Config<Self::App>) -> Self::Config;
    /// Starts it. Returns once it's ready to answer.
    fn start(
        ctx: &<Self::App as App>::Context,
        sources: &Self::Sources,
        config: &Self::Config,
        features: &Features<Self::App>,
    ) -> Arc<Self>;
    /// The settings changed while it runs, from options `before` to
    /// `config`: applies them in place, or asks to be stopped and started
    /// again (the default when its options changed). A kept module is told
    /// right after it starts too, and always applies them in place.
    fn reconfigure(&self, before: &Self::Config, config: &Self::Config, features: &Features<Self::App>) -> Reconfigure {
        let _ = features;
        if before == config { Reconfigure::InPlace } else { Reconfigure::Restart }
    }
    /// After every change, with the modules running now, for one that uses
    /// another's service.
    fn linked(&self, running: &<Self::App as App>::Running) {
        let _ = running;
    }
    /// Sends a new subscriber to [`Module::TOPIC`] the current state.
    fn send_state(&self, client: <Self::App as App>::Client) {
        let _ = client;
    }
    /// The client `client` hung up.
    fn client_gone(&self, client: <Self::App as App>::Client) {
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

/// A running module and the options it runs with.
pub struct Slot<M: Module> {
    pub module: Arc<M>,
    pub config: M::Config,
}

/// A module of the registry, reached by its type.
pub trait Has<M: Module> {
    fn module(&self) -> Option<&Arc<M>>;
}

/// Starts `M`, logged.
pub fn start<M: Module>(
    ctx: &<M::App as App>::Context,
    sources: &M::Sources,
    config: &Config<M::App>,
    features: &Features<M::App>,
) -> Slot<M> {
    <M::App as App>::logger(ctx).info(LOG_SOURCE, &format!("starting {}", M::ID));
    let config = M::config(config);
    Slot { module: M::start(ctx, sources, &config, features), config }
}

/// `slot`'s module after a change: kept while its feature is ready and it
/// applies the change in place, else stopped; started while its feature is
/// ready and none is kept.
pub fn reconcile<M: Module>(
    slot: &mut Option<Slot<M>>,
    ctx: &<M::App as App>::Context,
    sources: &M::Sources,
    config: &Config<M::App>,
    features: &Features<M::App>,
) {
    let ready = <M::App as App>::is_ready(features, M::ID);
    if let Some(running) = slot.take() {
        let options = M::config(config);
        if ready && running.module.reconfigure(&running.config, &options, features) == Reconfigure::InPlace {
            *slot = Some(Slot { module: running.module, config: options });
            return;
        }
        running.module.stop();
        <M::App as App>::logger(ctx).info(LOG_SOURCE, &format!("stopped {}", M::ID));
    }
    if ready {
        *slot = Some(start(ctx, sources, config, features));
    }
}

/// A module kept for the app's life after a change: told the settings,
/// which it applies in place.
pub fn keep<M: Module>(slot: &mut Slot<M>, config: &Config<M::App>, features: &Features<M::App>) {
    let options = M::config(config);
    let applied = slot.module.reconfigure(&slot.config, &options, features);
    debug_assert_eq!(applied, Reconfigure::InPlace, "{} is kept, so it can't restart", M::ID);
    slot.config = options;
}

/// The registry of the app `$app`: modules that run while their feature is
/// ready, then the ones kept for the app's life. Within each, the order
/// modules start in. Generates `Sources`, `Running` and `Kept` where it's
/// called.
#[macro_export]
macro_rules! modules {
    (
        app: $app:ty,
        while_ready: [$($field:ident: $module:ty),* $(,)?],
        kept: [$($kept:ident: $kept_module:ty),* $(,)?] $(,)?
    ) => {
        /// Where each module reads and keeps things.
        #[derive(Clone)]
        pub struct Sources {
            pub apps: <$app as $crate::App>::AppSources,
            $(pub $field: <$module as $crate::Module>::Sources,)*
            $(pub $kept: <$kept_module as $crate::Module>::Sources,)*
        }

        impl Sources {
            pub fn system(env: &<$app as $crate::App>::Env<'_>) -> Self {
                Self {
                    apps: <$app as $crate::App>::app_sources(env),
                    $($field: <$module as $crate::Module>::sources(env),)*
                    $($kept: <$kept_module as $crate::Module>::sources(env),)*
                }
            }
        }

        /// The modules running.
        pub struct Running {
            $($field: Option<$crate::Slot<$module>>,)*
            $($kept: $crate::Slot<$kept_module>,)*
        }

        impl Running {
            /// Every module's feature id.
            #[cfg(test)]
            pub const IDS: &[&str] = &[$(<$module as $crate::Module>::ID,)* $(<$kept_module as $crate::Module>::ID,)*];

            /// Every module's topic, if it has one ([`Module::TOPIC`]).
            #[cfg(test)]
            pub const TOPICS: &[Option<<$app as $crate::App>::Topic>] =
                &[$(<$module as $crate::Module>::TOPIC,)* $(<$kept_module as $crate::Module>::TOPIC,)*];

            /// The module `M`, while it runs.
            pub fn get<M: $crate::Module>(&self) -> Option<std::sync::Arc<M>>
            where
                Self: $crate::Has<M>,
            {
                $crate::Has::<M>::module(self).cloned()
            }

            /// Starts the modules kept for the app's life, and those whose
            /// feature is ready.
            pub fn start(
                ctx: &<$app as $crate::App>::Context,
                sources: &Sources,
                config: &$crate::Config<$app>,
                features: &$crate::Features<$app>,
            ) -> Self {
                let mut running = Self {
                    $($field: None,)*
                    $($kept: $crate::start(ctx, &sources.$kept, config, features),)*
                };
                running.apply(ctx, sources, config, features);
                running
            }

            /// Starts, stops, restarts and reconfigures modules to match
            /// `config` and `features`, then links them up.
            pub fn apply(
                &mut self,
                ctx: &<$app as $crate::App>::Context,
                sources: &Sources,
                config: &$crate::Config<$app>,
                features: &$crate::Features<$app>,
            ) {
                let apps_needed = false
                    $(|| (<$module as $crate::Module>::NEEDS_APPS
                        && <$app as $crate::App>::is_ready(features, <$module as $crate::Module>::ID)))*;
                // Followed before a module that names apps starts, and
                // forgotten once the last of them has stopped.
                if apps_needed {
                    <$app as $crate::App>::follow_apps(ctx);
                }
                $($crate::reconcile(&mut self.$field, ctx, &sources.$field, config, features);)*
                $($crate::keep(&mut self.$kept, config, features);)*
                if !apps_needed {
                    <$app as $crate::App>::forget_apps(ctx);
                }
                $(if let Some(slot) = &self.$field { $crate::Module::linked(&*slot.module, self); })*
                $($crate::Module::linked(&*self.$kept.module, self);)*
            }

            /// Sends `client` the current state for `topic`.
            pub fn send_state(&self, topic: <$app as $crate::App>::Topic, client: <$app as $crate::App>::Client) {
                $(if <$module as $crate::Module>::TOPIC == Some(topic) && let Some(slot) = &self.$field {
                    $crate::Module::send_state(&*slot.module, client);
                })*
                $(if <$kept_module as $crate::Module>::TOPIC == Some(topic) {
                    $crate::Module::send_state(&*self.$kept.module, client);
                })*
            }

            /// Tells every module running that `client` hung up.
            pub fn client_gone(&self, client: <$app as $crate::App>::Client) {
                $(if let Some(slot) = &self.$field { $crate::Module::client_gone(&*slot.module, client); })*
                $($crate::Module::client_gone(&*self.$kept.module, client);)*
            }

            /// Stops every module, for an app that ends without exiting the
            /// process.
            pub fn stop_all(&mut self) {
                $(if let Some(slot) = self.$field.take() { $crate::Module::stop(&*slot.module); })*
                $($crate::Module::stop(&*self.$kept.module);)*
            }
        }

        $(impl $crate::Has<$module> for Running {
            fn module(&self) -> Option<&std::sync::Arc<$module>> {
                self.$field.as_ref().map(|slot| &slot.module)
            }
        })*
        /// The modules kept for the app's life.
        #[derive(Clone)]
        pub struct Kept {
            $(pub $kept: std::sync::Arc<$kept_module>,)*
        }

        impl Running {
            /// The modules kept for the app's life, for reaching them
            /// without the settings lock.
            pub fn kept(&self) -> Kept {
                Kept { $($kept: std::sync::Arc::clone(&self.$kept.module),)* }
            }
        }

        $(impl $crate::Has<$kept_module> for Running {
            fn module(&self) -> Option<&std::sync::Arc<$kept_module>> {
                Some(&self.$kept.module)
            }
        })*
    };
}
