use nova_session_bus::Bus;
use std::sync::Arc;

use nova_x11_tray::FEATURE_ID;

pub struct X11Tray(Arc<nova_x11_tray::X11Tray>);

impl nova_modules::Module for X11Tray {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    type Sources = Bus;
    type Config = ();

    fn sources(_: &crate::modules::Env) -> Bus {
        Bus::Session
    }

    fn config(_: &crate::config::Config) {}

    fn start(ctx: &crate::modules::Context, bus: &Bus, (): &(), _: &[nova_daemon_info::FeatureState]) -> Arc<Self> {
        Arc::new(Self(nova_x11_tray::X11Tray::start(bus.clone(), &ctx.path_var, Arc::clone(&ctx.logger))))
    }

    fn stop(&self) {
        self.0.stop();
    }
}
