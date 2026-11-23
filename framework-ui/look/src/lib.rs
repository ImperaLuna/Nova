//! Nova's look, for everything the GPUI program draws. Each piece is
//! defined once here and every feature's ui draws with it, so the shell
//! reads as one thing:
//!
//! - [`tokens`]: every size, opacity and timing, named, and the font
//!   families ([`Family`]: the face this system draws each with).
//! - [`Theme`]: the active theme's colors and the fills derived from them.
//!   Views read it while they render ([`Theme::of`]); [`Theme::set`] swaps
//!   it and redraws, so a theme change needs nothing from a view.
//! - [`LookStyled`]: the running text style, and rounded corners that hold
//!   their content in (GPUI clips to a rounded parent's box, not its
//!   corners).
//! - [`Icon`]: the line icons ([`Glyph`]), which need [`IconAssets`] as the
//!   app's asset source.
//! - [`AppIcon`]: an app's icon from its image file, or its letter on a
//!   swatch.
//! - [`TextField`]: a one-line text field ([`FieldEvent`] when it changes).
//! - [`Segments`]: a row of tabs, one of them picked.
//! - Quick settings' controls, which other features fill it with:
//!   [`Tile`], [`Slider`] (with [`VolumeGlyph`] for a volume),
//!   [`ListRow`] and a page's [`PageHeader`] (with [`header_inset`]), and
//!   the small pieces their pages share: [`ToggleSwitch`], [`RoundButton`],
//!   [`PromptButton`], [`CardTitle`], a device's [`DeviceBattery`] and a [`graph`] over
//!   time. [`PillButton`] is a round button in a pill.
//! - [`Spinner`]: the loading spinner, and the dashed ring of something
//!   that isn't there.
//! - [`Glass`]: the smoked glass pills and panels are made of, with its
//!   edge and its shadow.
//! - [`Shape`], [`set_blur_region`]: what a window draws, as the region
//!   the compositor blurs behind and takes clicks on.
//! - [`motion`]: the panel curve and the spring. [`Wheel`]: a mouse
//!   wheel's notches. [`fade_color`], [`fade`] and [`Hovered`]: the short
//!   fades of a hover wash or a fill, kept by an element's id.
//! - [`bytes`]: sizes in bytes as people read them.
//! - [`ShellWindow`]: the options every shell window opens with. No other
//!   crate builds `WindowOptions`.
//!
//! `examples/sample.rs` draws a pill and a panel with all of it, to look at
//! and to screenshot.

mod app_icon;
pub mod bytes;
mod card_title;
mod device_battery;
mod fade;
mod family;
mod field;
mod glass;
pub mod graph;
mod icon;
mod list_row;
pub mod motion;
mod page_header;
mod pill_button;
mod prompt_button;
mod round_button;
mod segments;
mod shape;
mod slider;
mod spinner;
mod style;
mod theme;
mod tile;
mod toggle_switch;
pub mod tokens;
mod volume_icon;
mod wheel;
mod window;

pub use app_icon::AppIcon;
pub use card_title::CardTitle;
pub use device_battery::DeviceBattery;
pub use fade::{Hovered, fade, fade_color, keyed};
pub use family::Family;
pub use field::{Editing, FieldEvent, TextField};
pub use glass::Glass;
pub use icon::{Glyph, Icon, IconAssets};
pub use list_row::ListRow;
pub use page_header::{PILL_BUTTONS, PageHeader, header_inset};
pub use pill_button::PillButton;
pub use prompt_button::PromptButton;
pub use round_button::RoundButton;
pub use segments::Segments;
pub use shape::{Shape, region, set_blur_region};
pub use slider::Slider;
pub use spinner::Spinner;
pub use style::{LookStyled, Touching};
pub use theme::{Theme, mix};
pub use tile::Tile;
pub use toggle_switch::ToggleSwitch;
pub use volume_icon::{OutputKind, VolumeGlyph};
pub use wheel::{WHEEL_LINE, Wheel, scrolled};
pub use window::{APP_ID, IGNORE_RESERVED_SPACE, NAMESPACE, NAMESPACE_PREFIX, ShellWindow};
