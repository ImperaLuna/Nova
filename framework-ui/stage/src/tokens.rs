//! The stage's own values: the ones no other crate draws with. Everything
//! shared is `nova_look::tokens`.

use gpui::{Pixels, px};
use std::time::Duration;

/// How often the stage looks for screens that came or went: GPUI has no
/// event for it.
pub const SCREEN_POLL: Duration = Duration::from_secs(1);

/// A panel that grows out of nothing (its pill isn't on screen) starts this
/// far above its place and settles down into it as it fades in. The
/// launcher's `launcherEnterRise` in today's shell.
pub const PANEL_ENTER_RISE: Pixels = px(8.0);

/// A pill's content is gone once its panel has grown this share of the way.
pub const PILL_FADE_OUT: f32 = 0.3;

/// What a screen that doesn't say how fast it refreshes is taken to do.
pub const FALLBACK_REFRESH: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// A frame is late when it comes more than this many refreshes after the
/// one before.
pub const LATE_FRAME_REFRESHES: f32 = 1.5;

/// How often the frame counter reports.
pub const FRAME_REPORT: Duration = Duration::from_secs(1);

/// The stage's layer surface, as compositor rules name it.
pub const STAGE_NAMESPACE: &str = "nova-stage";
/// The surface that takes clicks off an open panel.
pub const CATCHER_NAMESPACE: &str = "nova-click-catcher";
/// The empty surface that keeps the bar's room free of app windows.
pub const BAR_ROOM_NAMESPACE: &str = "nova-exclusion-zone";
/// A layer surface needs some height; the bar's room is this tall and
/// draws nothing.
pub const BAR_ROOM_HEIGHT: Pixels = px(1.0);
