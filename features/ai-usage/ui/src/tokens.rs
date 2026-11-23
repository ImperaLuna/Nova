//! The AI usage feature's own values: the ones nothing else draws with.
//! Everything shared is `nova_look::tokens`.

use std::time::Duration;

/// The `%` after the panel's headline number, as a share of its size.
pub const HERO_UNIT_SCALE: f32 = 0.42;

/// How often the "left" and "resets" texts count down while the panel is
/// open.
pub const COUNTDOWN: Duration = Duration::from_secs(30);
