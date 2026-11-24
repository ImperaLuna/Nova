use crate::config::Config;
use crate::{
    ai_usage, bluetooth, features, launcher, media, network, notifications, nova_files, peripherals, recording,
    resources, screenshot, session_lock, sound, tray, wallpaper, x11_tray,
};
use nova_apps::AppIndex;
use nova_carrier::Topic;
use nova_compositor::Compositor;
use nova_daemon_info::FeatureState;
use nova_hub::{ClientId, Hub};
use nova_icons::Icons;
use nova_log::Logger;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Env<'a> {
    pub args: &'a crate::Args,
    pub runtime_dir: &'a Path,
    pub path_var: &'a str,
    pub icons: &'a Arc<Icons>,
    pub home: Option<PathBuf>,
}

pub struct Context {
    pub hub: Arc<Hub>,
    pub logger: Arc<Logger>,
    pub path_var: String,
    pub icons: Arc<Icons>,
    pub compositor: Option<Arc<Compositor>>,
    pub apps: Arc<AppIndex>,
}

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
        sound: sound::Sound,
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

    #[test]
    fn every_module_is_a_daemon_feature() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../features");
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
