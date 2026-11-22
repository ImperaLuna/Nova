//! Runs the X11 tray feature (`nova-x11-tray`, in `features/x11-tray`) in
//! novad: its module. Wiring only: it adapts the feature's core to novad's
//! `Module` trait, and all of the feature's work and state stay in the
//! core. It has no commands and no topic: its icons reach whatever tray is
//! up.

use nova_session_bus::Bus;
use std::sync::Arc;

use nova_x11_tray::FEATURE_ID;

/// The feature's core, as novad's module.
pub struct X11Tray(Arc<nova_x11_tray::X11Tray>);

/// No options. Its icons reach whatever tray is up, on the tray's bus.
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
