//! The volume's icon, for an output: a solid speaker with no, one or two
//! waves, crossed out while muted, struck through with no output,
//! headphones with solid cups, the Bluetooth rune or a monitor (HDMI or
//! DisplayPort audio). Quick settings' sound slider draws it, and the
//! volume capsule will.

use crate::{Glyph, Icon};

/// Which volume icon (see the module).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeGlyph {
    /// No output at all.
    None,
    Muted,
    /// Speakers at no volume, below half and from half up.
    Off,
    Low,
    High,
    Headphones,
    Bluetooth,
    Display,
}

/// What an output is, for its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Speakers,
    Headphones,
    Bluetooth,
    Display,
}

impl VolumeGlyph {
    /// The icon for the output in use (`None` for none), muted or at
    /// `volume` (1 is 100%). Only speakers show the level.
    pub fn pick(output: Option<OutputKind>, muted: bool, volume: f32) -> Self {
        match output {
            None => Self::None,
            Some(_) if muted => Self::Muted,
            Some(OutputKind::Headphones) => Self::Headphones,
            Some(OutputKind::Bluetooth) => Self::Bluetooth,
            Some(OutputKind::Display) => Self::Display,
            Some(OutputKind::Speakers) if volume <= 0.0 => Self::Off,
            Some(OutputKind::Speakers) if volume < 0.5 => Self::Low,
            Some(OutputKind::Speakers) => Self::High,
        }
    }

    /// Its icon at the panel icon size.
    pub fn icon(self) -> Icon {
        match self {
            Self::Headphones => Icon::new(Glyph::HEADPHONES_BAND).filled(Glyph::HEADPHONES_CUPS),
            Self::Bluetooth => Icon::new(Glyph::BLUETOOTH),
            Self::Display => Icon::new(Glyph::MONITOR),
            // The speaker's body, solid, with what is drawn beside it.
            Self::Off => Icon::new(Glyph::SPEAKER_BODY).filled(Glyph::SPEAKER_BODY),
            Self::Low => Icon::new(Glyph::WAVE).filled(Glyph::SPEAKER_BODY),
            Self::High => Icon::new(Glyph::WAVES).filled(Glyph::SPEAKER_BODY),
            Self::Muted => Icon::new(Glyph::MUTED).filled(Glyph::SPEAKER_BODY),
            Self::None => Icon::new(Glyph::STRIKE).filled(Glyph::SPEAKER_BODY),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_picks_as_volume_js_does() {
        use OutputKind::*;
        assert_eq!(VolumeGlyph::pick(None, false, 0.5), VolumeGlyph::None);
        assert_eq!(VolumeGlyph::pick(None, true, 0.5), VolumeGlyph::None, "no output wins over muted");
        assert_eq!(VolumeGlyph::pick(Some(Headphones), true, 0.5), VolumeGlyph::Muted);
        assert_eq!(VolumeGlyph::pick(Some(Headphones), false, 0.0), VolumeGlyph::Headphones);
        assert_eq!(VolumeGlyph::pick(Some(Display), false, 0.2), VolumeGlyph::Display);
        assert_eq!(VolumeGlyph::pick(Some(Speakers), false, 0.0), VolumeGlyph::Off);
        assert_eq!(VolumeGlyph::pick(Some(Speakers), false, 0.49), VolumeGlyph::Low);
        assert_eq!(VolumeGlyph::pick(Some(Speakers), false, 0.5), VolumeGlyph::High);
        assert_eq!(VolumeGlyph::pick(Some(Speakers), false, 1.4), VolumeGlyph::High);
    }
}
