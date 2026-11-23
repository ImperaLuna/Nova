//! What `volume.js` worked out for the capsule, with no GPUI in it: the
//! output it shows, when a change is worth showing, where the capsule is
//! drawn on each frame, and what a point on it sets the volume to.

use crate::tokens::{EDGE_GAP, HEIGHT, LABEL_FADE_IN, OVERSHOOT, STRETCH, STRETCH_SPEED, WALL, WIDTH};
use nova_look::OutputKind;
use nova_sound_api::{DeviceKind, Sound};

/// The output in use, as the capsule shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Output {
    pub id: u32,
    /// 1 is 100%; another program can set more.
    pub volume: f32,
    pub muted: bool,
    pub kind: OutputKind,
}

/// The output in use, while the sound server answers and there is one.
pub fn output(sound: Option<&Sound>) -> Option<Output> {
    let sound = sound.filter(|sound| sound.available)?;
    let device = sound.outputs.iter().find(|device| device.current)?;
    let kind = match device.kind {
        DeviceKind::Headphones => OutputKind::Headphones,
        DeviceKind::Bluetooth => OutputKind::Bluetooth,
        DeviceKind::Display => OutputKind::Display,
        DeviceKind::Speakers => OutputKind::Speakers,
    };
    Some(Output { id: device.id, volume: device.volume, muted: device.muted, kind })
}

/// Whether the output in use going from `before` to `now` is a change to
/// show. The first one seen is where things start, not a change, so
/// starting Nova shows nothing; another output is a new one, which shows.
/// An output that goes shows nothing: there is nothing in use to show.
pub fn is_change(before: Option<Output>, now: Option<Output>) -> bool {
    match (before, now) {
        (Some(before), Some(now)) => before.id != now.id || before.volume != now.volume || before.muted != now.muted,
        _ => false,
    }
}

/// How full the capsule is for `output`: full above 100%, empty without
/// one.
pub fn level(output: Option<Output>) -> f32 {
    output.map_or(0.0, |output| output.volume.clamp(0.0, 1.0))
}

/// The number on the capsule: the percent, or "–" without an output.
pub fn label(output: Option<Output>) -> String {
    output.map_or_else(|| "–".into(), |output| format!("{}", (output.volume * 100.0).round() as i64))
}

/// Where the capsule is drawn, in its window's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapsuleBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub radius: f32,
}

/// The tallest the capsule gets, stretched and overshooting.
pub fn tallest() -> f32 {
    f32::from(HEIGHT) * (1.0 + OVERSHOOT / 2.0) * (1.0 + STRETCH)
}

/// Where the capsule is drawn in a window `width` x `height` against the
/// right edge, `open` (0 in, 1 out, overshooting) and moving at `speed`
/// (its way out per second). It grows from half its height as it comes
/// out, and stretches thinner and taller while it moves fast.
pub fn capsule_box(open: f32, speed: f32, width: f32, height: f32) -> CapsuleBox {
    let (full_width, full_height, gap) = (f32::from(WIDTH), f32::from(HEIGHT), f32::from(EDGE_GAP));
    let out = open.max(0.0);
    let stretch = 1.0 + STRETCH * (speed.abs() / STRETCH_SPEED).min(1.0);
    let h = full_height * (0.5 + 0.5 * out.min(1.0 + OVERSHOOT)) * stretch;
    let w = full_width / stretch;
    CapsuleBox {
        x: width - out * (full_width + gap) + (full_width - w) / 2.0,
        y: (height - h) / 2.0,
        w,
        h,
        radius: w / 2.0,
    }
}

/// The label's opacity: it fades in over the last stretch of coming out.
pub fn label_opacity(open: f32) -> f32 {
    (1.0 - (1.0 - open) / LABEL_FADE_IN).clamp(0.0, 1.0)
}

/// The liquid's surface, from the top of a capsule `height` tall, at
/// `level`. Empty, it's the bottom.
pub fn surface_y(level: f32, height: f32) -> f32 {
    let wall = f32::from(WALL);
    if level <= 0.0 {
        return height;
    }
    wall + (1.0 - level) * (height - 2.0 * wall)
}

/// The volume for a point `y` down a capsule `height` tall, to the
/// percent.
pub fn volume_at(y: f32, height: f32) -> f32 {
    let wall = f32::from(WALL);
    let fraction = 1.0 - (y - wall) / (height - 2.0 * wall).max(1.0);
    (fraction.clamp(0.0, 1.0) * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_sound_api::Device;

    fn speakers(volume: f32, muted: bool) -> Output {
        Output { id: 1, volume, muted, kind: OutputKind::Speakers }
    }

    #[test]
    fn the_output_shown_is_the_one_in_use_while_the_server_answers() {
        let device = |id, current| Device {
            id,
            name: format!("device {id}"),
            kind: DeviceKind::Headphones,
            link: None,
            volume: 0.3,
            muted: true,
            current,
        };
        let sound = Sound { available: true, outputs: vec![device(1, false), device(2, true)], ..Sound::default() };
        let shown = output(Some(&sound)).unwrap();
        assert_eq!((shown.id, shown.kind, shown.muted), (2, OutputKind::Headphones, true));
        assert_eq!(output(Some(&Sound { available: false, ..sound })), None);
        assert_eq!(output(None), None);
    }

    #[test]
    fn a_change_shows_once_things_have_started() {
        assert!(!is_change(None, Some(speakers(0.4, false))), "the first state is where things start");
        assert!(is_change(Some(speakers(0.4, false)), Some(speakers(0.45, false))));
        assert!(is_change(Some(speakers(0.4, false)), Some(speakers(0.4, true))));
        let other = Output { id: 2, ..speakers(0.4, false) };
        assert!(is_change(Some(speakers(0.4, false)), Some(other)), "another output is news");
        assert!(!is_change(Some(speakers(0.4, false)), Some(speakers(0.4, false))));
        assert!(!is_change(Some(speakers(0.4, false)), None), "an output that went shows nothing");
    }

    #[test]
    fn the_level_and_number_follow_the_volume() {
        assert_eq!(level(Some(speakers(1.3, false))), 1.0, "full above 100%");
        assert_eq!(level(None), 0.0);
        assert_eq!(label(Some(speakers(0.456, false))), "46");
        assert_eq!(label(Some(speakers(1.3, false))), "130");
        assert_eq!(label(None), "–");
    }

    #[test]
    fn it_comes_out_from_the_edge_at_half_its_height() {
        let (width, height) = (100.0, tallest());
        let hidden = capsule_box(0.0, 0.0, width, height);
        assert_eq!((hidden.x, hidden.h), (width, f32::from(HEIGHT) / 2.0));
        let out = capsule_box(1.0, 0.0, width, height);
        assert_eq!(out.x + out.w + f32::from(EDGE_GAP), width);
        assert_eq!((out.h, out.radius), (f32::from(HEIGHT), f32::from(WIDTH) / 2.0));
        assert!((out.y + out.h / 2.0 - height / 2.0).abs() < 1e-3, "centered");
        // Fast, it stretches thinner and taller, at most by STRETCH.
        let fast = capsule_box(1.0, 100.0, width, height);
        assert!((fast.h / out.h - (1.0 + STRETCH)).abs() < 1e-5 && fast.w < out.w);
        // Overshooting it grows past its full height, and never past the
        // window's.
        let over = capsule_box(1.5, 100.0, width, height);
        assert!(over.h > out.h && over.h <= height + 1e-3, "{}", over.h);
    }

    #[test]
    fn the_label_fades_in_over_the_last_of_the_way_out() {
        assert_eq!(label_opacity(0.0), 0.0);
        assert!(label_opacity(1.0 - LABEL_FADE_IN) < 1e-5);
        assert!((label_opacity(1.0 - LABEL_FADE_IN / 2.0) - 0.5).abs() < 1e-5);
        assert_eq!(label_opacity(1.0), 1.0);
        assert_eq!(label_opacity(1.05), 1.0);
    }

    #[test]
    fn a_point_on_it_sets_the_volume_from_the_wall_up() {
        let height = f32::from(HEIGHT);
        let wall = f32::from(WALL);
        assert_eq!(volume_at(wall, height), 1.0);
        assert_eq!(volume_at(height - wall, height), 0.0);
        assert_eq!(volume_at(height / 2.0, height), 0.5);
        assert_eq!(volume_at(-20.0, height), 1.0);
        assert_eq!(surface_y(1.0, height), wall);
        assert_eq!(surface_y(0.0, height), height);
        assert_eq!(surface_y(0.5, height), height / 2.0);
    }
}
