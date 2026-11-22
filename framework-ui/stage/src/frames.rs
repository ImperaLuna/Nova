//! Counting frames: how many each motion on a stage drew and how many came
//! late, and how many a window draws while nothing should move (a line at
//! most each second, and none while it draws nothing).
//! Both only log (novad keeps the UI's lines), and only when asked for
//! (`StageOptions::log_frames`): they are how a port thread or the
//! checkpoint sees that a pill at rest costs no frames.
//!
//! These count the frames GPUI rendered. What reached the screen is counted
//! from the surface's commits (`WAYLAND_DEBUG=client`), as the prototype
//! did.

use crate::tokens::{FRAME_REPORT, LATE_FRAME_REFRESHES};
use std::time::{Duration, Instant};

/// The frames of one stage window.
#[derive(Debug)]
pub struct FrameLog {
    /// The screen it is on, as the log names it.
    screen: String,
    refresh: Duration,
    motion: Motion,
    motions: u32,
    total: Totals,
    /// Frames drawn since the last report, and when that was.
    drawn: u32,
    reported: Instant,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Totals {
    frames: u64,
    late: u64,
}

/// The frames of the motion under way.
#[derive(Debug, Default)]
struct Motion {
    previous: Option<Instant>,
    frames: u32,
    late: u32,
    worst: Duration,
}

impl FrameLog {
    pub fn new(screen: String, refresh: Duration, now: Instant) -> Self {
        Self {
            screen,
            refresh,
            motion: Motion::default(),
            motions: 0,
            total: Totals::default(),
            drawn: 0,
            reported: now,
        }
    }

    /// A frame was rendered at `now`; `moving` says whether something on
    /// the stage was still on its way. Returns the lines to log.
    pub fn frame(&mut self, moving: bool, now: Instant) -> Vec<String> {
        let mut lines = Vec::new();
        let since = now.saturating_duration_since(self.reported);
        if since >= FRAME_REPORT {
            // The frames up to this one. A stage at rest draws none, so
            // the line says how long they took: no line comes meanwhile.
            lines.push(format!("frames screen={} drawn={} in={:.1}s", self.screen, self.drawn, since.as_secs_f64()));
            self.drawn = 0;
            self.reported = now;
        }
        self.drawn += 1;

        let in_motion = moving || self.motion.frames > 0;
        if in_motion {
            if let Some(previous) = self.motion.previous {
                let gap = now.saturating_duration_since(previous);
                self.motion.worst = self.motion.worst.max(gap);
                if gap > self.refresh.mul_f32(LATE_FRAME_REFRESHES) {
                    self.motion.late += 1;
                }
            }
            self.motion.frames += 1;
            self.motion.previous = Some(now);
        }
        if in_motion && !moving {
            lines.push(self.settle());
        }
        lines
    }

    /// The motion's line, once it has arrived.
    fn settle(&mut self) -> String {
        let motion = std::mem::take(&mut self.motion);
        self.motions += 1;
        self.total.frames += u64::from(motion.frames);
        self.total.late += u64::from(motion.late);
        format!(
            "frames screen={} motion={} drawn={} late={} worst={:.1}ms refresh={:.2}ms total_drawn={} total_late={} ({:.2}%)",
            self.screen,
            self.motions,
            motion.frames,
            motion.late,
            motion.worst.as_secs_f64() * MILLIS,
            self.refresh.as_secs_f64() * MILLIS,
            self.total.frames,
            self.total.late,
            PERCENT * self.total.late as f64 / self.total.frames.max(1) as f64,
        )
    }
}

const MILLIS: f64 = 1000.0;
const PERCENT: f64 = 100.0;

#[cfg(test)]
mod tests {
    use super::*;

    const REFRESH: Duration = Duration::from_millis(10);

    #[test]
    fn a_frame_is_late_past_one_and_a_half_refreshes() {
        let start = Instant::now();
        let mut log = FrameLog::new("DP-1".into(), REFRESH, start);
        let at = |millis| start + Duration::from_millis(millis);
        assert!(log.frame(true, at(0)).is_empty());
        assert!(log.frame(true, at(10)).is_empty());
        assert!(log.frame(true, at(26)).is_empty());
        let lines = log.frame(false, at(36));
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("motion=1 drawn=4 late=1 worst=16.0ms"), "{}", lines[0]);
        assert!(lines[0].contains("total_drawn=4 total_late=1 (25.00%)"), "{}", lines[0]);
    }

    #[test]
    fn frames_at_rest_are_counted_per_second_and_belong_to_no_motion() {
        let start = Instant::now();
        let mut log = FrameLog::new("DP-1".into(), REFRESH, start);
        assert!(log.frame(false, start).is_empty());
        assert!(log.frame(false, start + Duration::from_millis(500)).is_empty());
        let lines = log.frame(false, start + Duration::from_millis(1200));
        assert_eq!(lines, ["frames screen=DP-1 drawn=2 in=1.2s"]);
        assert_eq!(log.total, Totals::default());
    }
}
