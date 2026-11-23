use crate::Theme;
use crate::tokens::{FONT_SANS, FONT_SIZE, LINE_HEIGHT, WEIGHT_BODY};
use gpui::{App, Pixels, Styled, px, relative};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touching {
    All,
    Top,
    Bottom,
    Left,
    Right,
}

pub trait LookStyled: Styled + Sized {
    fn body_text(self, cx: &App) -> Self {
        self.font_family(FONT_SANS)
            .text_size(FONT_SIZE)
            .font_weight(WEIGHT_BODY)
            .line_height(relative(LINE_HEIGHT))
            .text_color(Theme::of(cx).fg)
    }

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
