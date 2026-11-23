//! The stage: one see-through window across the top of each screen that
//! pills and panels are drawn on, so a pill can grow into its panel. It
//! takes clicks only where something is drawn; everywhere else they reach
//! the windows below. It names no feature: each feature's ui crate
//! registers its views, and the stage shows those the manifests' slots ask
//! for (`features/<id>/feature.toml`, `[[slot]]`), for the features that
//! are ready.
//!
//! A feature's ui has one `pub fn register(cx: &mut App)`, which `nova-ui`
//! calls before [`Stage::start`]:
//!
//! ```ignore
//! pub fn register(cx: &mut App) {
//!     // A pill in a bar row (hosts bar.left, bar.center, bar.right).
//!     nova_stage::pill("clock.pill", cx, |screen, cx| Pill::new(cx.new(|cx| ClockPill::new(screen, cx))));
//!     // A pill that grows into a panel on a click.
//!     nova_stage::pill("media.pill", cx, |_, cx| {
//!         Pill::new(cx.new(MediaPill::new)).panel(Panel::new("media", size(WIDTH, HEIGHT), cx.new(MediaPanel::new)))
//!     });
//!     // A panel that is no pill's own (host stage.panels), out of another pill.
//!     nova_stage::panel("launcher.panel", cx, |_, cx| {
//!         let launcher = cx.new(Launcher::new);
//!         let field = launcher.read(cx).field.clone();
//!         Panel::new("launcher", size(WIDTH, HEIGHT), launcher).from("central-pill").takes_typing(field)
//!     });
//! }
//! ```
//!
//! The views are ordinary GPUI views that draw content only. The stage
//! draws the glass, lays out the bar's rows, runs the morph, sets the blur
//! and click regions from what it drew, holds the keyboard while a panel
//! is open and closes panels on Esc and a click outside
//! ([`Stage::toggle`] and friends open them from code).
//!
//! - [`Pill`], [`Panel`]: what a feature hands over, with the contract's
//!   flags.
//! - [`pill`], [`panel`], [`part`], [`overlay`]: registering a view by its
//!   id. [`Parts`] shows the views of a host a feature draws itself.
//! - [`Stage`]: starting it, and opening, closing and resizing panels.
//! - [`Screens`], [`Screen`]: the screens, looked for on a timer (GPUI has
//!   no event for one that comes or goes).
//! - [`Slots`]: which views the ready features' slots ask for.
//! - `rules`, `geometry`, `reveal`, `kept`, `aside`: the logic without drawing,
//!   each tested alone. `view` and `draw` are one screen's stage.

mod aside;
mod draw;
mod frames;
mod geometry;
mod item;
mod kept;
mod registry;
mod reveal;
mod rules;
mod satellites;
mod screens;
mod slots;
mod stage;
mod tokens;
mod view;

pub use item::{Panel, Pill};
pub use registry::{Parts, overlay, panel, part, pill};
pub use screens::{Screen, Screens};
pub use slots::{HOST_BAR_CENTER, HOST_BAR_LEFT, HOST_BAR_RIGHT, HOST_SHELL_OVERLAYS, HOST_STAGE_PANELS, Slots};
pub use stage::{Stage, StageOptions};
