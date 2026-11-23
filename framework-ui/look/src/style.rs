//! Styling every element shares: Nova's running text, and rounded corners
//! that hold their content in.

use crate::Theme;
use crate::tokens::{FONT_SANS, FONT_SIZE, LINE_HEIGHT, WEIGHT_BODY};
use gpui::{App, Pixels, Styled, px, relative};

/// Which of a rounded parent's edges an element reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touching {
    /// It fills the parent, or reaches all four corners.
    All,
    Top,
    Bottom,
    Left,
    Right,
}

/// Nova's styles on any GPUI element.
pub trait LookStyled: Styled + Sized {
    /// Nova's running text: the sans face at body size and weight, in the
    /// theme's ink, with lines as tall as Qt makes them. Set it once on
    /// what a window draws; text inside inherits it and changes only what
    /// differs (a size, a weight, a dimmer ink).
    fn body_text(self, cx: &App) -> Self {
        self.font_family(FONT_SANS)
            .text_size(FONT_SIZE)
            .font_weight(WEIGHT_BODY)
            .line_height(relative(LINE_HEIGHT))
            .text_color(Theme::of(cx).fg)
    }

    /// Rounds this element's corners so it stays inside a parent with
    /// corners of `parent_radius`, when it sits `inset` in from the edges
    /// it reaches. GPUI clips children to a rounded parent's box, not its
    /// corners, so a fill, a picture or a hover wash that reaches the edge
    /// would stick out of them.
    fn fit_corners(self, parent_radius: Pixels, inset: Pixels, touching: Touching) -> Self {
        let radius = (parent_radius - inset).max(px(0.0));
        match touching {
            Touching::All => self.rounded(radius),
            Touching::Top => self.rounded_t(radius),
            Touching::Bottom => self.rounded_b(radius),
            Touching::Left => self.rounded_l(radius),
            Touching::Right => self.rounded_r(radius),
        }
    }
}

impl<Element: Styled> LookStyled for Element {}
