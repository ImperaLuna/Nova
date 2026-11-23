use nova_log::Logger;
use std::sync::Arc;

const LOG_SOURCE: &str = "features";

pub fn not_ready(id: &str) -> String {
    format!("the {id} feature isn't ready (see `nova status`)")
}

pub trait App: Sized + 'static {
    type Env<'a>;
    type Config;
    type Context;
    type Feature;
    type Topic: PartialEq + Copy;
    type Client: Copy;
    type Running;
    type AppSources: Clone;

    fn app_sources(env: &Self::Env<'_>) -> Self::AppSources;
    fn is_ready(features: &[Self::Feature], id: &str) -> bool;
    fn logger(ctx: &Self::Context) -> &Logger;
    fn follow_apps(ctx: &Self::Context);
    fn forget_apps(ctx: &Self::Context);
}

pub type Config<A> = <A as App>::Config;
pub type Features<A> = [<A as App>::Feature];

pub trait Module: Send + Sync + Sized + 'static {
    type App: App;
    const ID: &'static str;
    const NEEDS_APPS: bool = false;
    const TOPIC: Option<<Self::App as App>::Topic> = None;
    type Sources: Clone;
    type Config: Clone + PartialEq;

    fn sources(env: &<Self::App as App>::Env<'_>) -> Self::Sources;
    fn config(config: &Config<Self::App>) -> Self::Config;
    fn start(
        ctx: &<Self::App as App>::Context,
        sources: &Self::Sources,
        config: &Self::Config,
        features: &Features<Self::App>,
    ) -> Arc<Self>;
    fn reconfigure(&self, before: &Self::Config, config: &Self::Config, features: &Features<Self::App>) -> Reconfigure {
        let _ = features;
        if before == config { Reconfigure::InPlace } else { Reconfigure::Restart }
    }
    fn linked(&self, running: &<Self::App as App>::Running) {
        let _ = running;
    }
    fn send_state(&self, client: <Self::App as App>::Client) {
        let _ = client;
    }
    fn client_gone(&self, client: <Self::App as App>::Client) {
        let _ = client;
    }
    fn stop(&self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconfigure {
    InPlace,
    Restart,
}

pub struct Slot<M: Module> {
    pub module: Arc<M>,
    pub config: M::Config,
}

pub trait Has<M: Module> {
    fn module(&self) -> Option<&Arc<M>>;
}

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

pub fn keep<M: Module>(slot: &mut Slot<M>, config: &Config<M::App>, features: &Features<M::App>) {
    let options = M::config(config);
    let applied = slot.module.reconfigure(&slot.config, &options, features);
    debug_assert_eq!(applied, Reconfigure::InPlace, "{} is kept, so it can't restart", M::ID);
    slot.config = options;
}

#[macro_export]
macro_rules! modules {
    (
        app: $app:ty,
        while_ready: [$($field:ident: $module:ty),* $(,)?],
        kept: [$($kept:ident: $kept_module:ty),* $(,)?] $(,)?
    ) => {
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

        pub struct Running {
            $($field: Option<$crate::Slot<$module>>,)*
            $($kept: $crate::Slot<$kept_module>,)*
        }

        impl Running {
            #[cfg(test)]
            pub const IDS: &[&str] = &[$(<$module as $crate::Module>::ID,)* $(<$kept_module as $crate::Module>::ID,)*];

            #[cfg(test)]
            pub const TOPICS: &[Option<<$app as $crate::App>::Topic>] =
                &[$(<$module as $crate::Module>::TOPIC,)* $(<$kept_module as $crate::Module>::TOPIC,)*];

            pub fn get<M: $crate::Module>(&self) -> Option<std::sync::Arc<M>>
            where
                Self: $crate::Has<M>,
            {
                $crate::Has::<M>::module(self).cloned()
            }

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

            pub fn send_state(&self, topic: <$app as $crate::App>::Topic, client: <$app as $crate::App>::Client) {
                $(if <$module as $crate::Module>::TOPIC == Some(topic) && let Some(slot) = &self.$field {
                    $crate::Module::send_state(&*slot.module, client);
                })*
                $(if <$kept_module as $crate::Module>::TOPIC == Some(topic) {
                    $crate::Module::send_state(&*self.$kept.module, client);
                })*
            }

            pub fn client_gone(&self, client: <$app as $crate::App>::Client) {
                $(if let Some(slot) = &self.$field { $crate::Module::client_gone(&*slot.module, client); })*
                $($crate::Module::client_gone(&*self.$kept.module, client);)*
            }

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
        #[derive(Clone)]
        pub struct Kept {
            $(pub $kept: std::sync::Arc<$kept_module>,)*
        }

        impl Running {
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
