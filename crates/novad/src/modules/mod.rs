//! novad's modules: every feature's daemon side behind the framework's
//! [`Module`](nova_modules::Module) trait (`nova-modules`, which runs them
//! by the settings), listed once in the registry below. The kept ones (a
//! recording to stop, the lock screen, the region picker, the wallpaper)
//! hold state across their feature turning off and on, so they run for
//! novad's whole life ([`Kept`]).
//!
//! The [`Context`] is what modules share: the hub, the logger, the
//! compositor, and the app index, which follows the apps while any module
//! that names them runs
//! ([`Module::NEEDS_APPS`](nova_modules::Module::NEEDS_APPS)).

use crate::config::Config;
use crate::{
    ai_usage, bluetooth, features, launcher, media, network, notifications, nova_files, peripherals, recording,
    resources, screenshot, session_lock, tray, wallpaper, x11_tray,
};
use nova_apps::AppIndex;
use nova_carrier::Topic;
use nova_compositor::Compositor;
use nova_hub::{ClientId, Hub};
use nova_icons::Icons;
use nova_log::Logger;
use nova_protocol::FeatureState;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

/// novad, as the app its modules run in.
pub struct Novad;

impl nova_modules::App for Novad {
    type Env<'a> = Env<'a>;
    type Config = Config;
    type Context = Context;
    type Feature = FeatureState;
    type Topic = Topic;
    type Client = ClientId;
    type Running = Running;
    type AppSources = nova_apps::Sources;

    fn app_sources(env: &Env) -> nova_apps::Sources {
        nova_apps::Sources::system(Arc::clone(env.icons))
    }

    fn is_ready(features: &[FeatureState], id: &str) -> bool {
        features::is_ready(features, id)
    }

    fn logger(ctx: &Context) -> &Logger {
        &ctx.logger
    }

    fn follow_apps(ctx: &Context) {
        ctx.apps.follow();
    }

    fn forget_apps(ctx: &Context) {
        ctx.apps.forget();
    }
}

nova_modules::modules! {
    app: Novad,
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
        nova_files: nova_files::NovaFiles,
    ],
    kept: [
        session_lock: session_lock::SessionLock,
        screenshots: screenshot::Screenshots,
        recordings: recording::Recordings,
        wallpaper: wallpaper::Wallpaper,
    ],
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
