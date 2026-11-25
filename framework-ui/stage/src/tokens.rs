use gpui::{Pixels, px};
use std::time::Duration;

// GPUI has no event for a screen that comes or goes: https://github.com/ImperaLuna/zed/tree/2033650502b1d0ec524b8913bf5a01b9463bed5d/crates/gpui
pub const SCREEN_POLL: Duration = Duration::from_secs(1);

pub const PANEL_ENTER_RISE: Pixels = px(8.0);

pub const PILL_FADE_OUT: f32 = 0.3;

pub const FALLBACK_REFRESH: Duration = Duration::from_nanos(1_000_000_000 / 60);

pub const LATE_FRAME_REFRESHES: f32 = 1.5;

pub const FRAME_REPORT: Duration = Duration::from_secs(1);

pub const STAGE_NAMESPACE: &str = "nova-stage";
pub const PANEL_NAMESPACE: &str = "nova-panel";
pub const PANEL_WINDOW_PARKED: Pixels = px(1.0);
pub const CATCHER_NAMESPACE: &str = "nova-click-catcher";
pub const BAR_ROOM_NAMESPACE: &str = "nova-exclusion-zone";
pub const BAR_ROOM_HEIGHT: Pixels = px(1.0);
