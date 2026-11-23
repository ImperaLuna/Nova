//! What keep awake draws, with GPUI: its tile in quick settings
//! (`quick-settings.tiles` slot `keep-awake.tile` of
//! `features/keep-awake/feature.toml`). `KeepAwakeTileSlot.qml` in
//! `ui/quick_settings/slots` in today's shell.
//!
//! While it's on, every screen's stage holds off idle
//! ([`Stage::keep_awake`]): no dimming, locking or sleeping. It lasts until
//! the UI restarts, as today's shell has it, and ends when the feature is
//! turned off.

use gpui::{App, AppContext, Context, IntoElement, Render, Subscription, Window};
use nova_look::{Glyph, Tile};
use nova_stage::Stage;

/// The view id the manifest's slot names.
const TILE: &str = "keep-awake.tile";
/// The host it is drawn in.
const HOST: &str = "quick-settings.tiles";

/// Registers the tile with the stage.
pub fn register(cx: &mut App) {
    nova_stage::part(TILE, cx, |_, cx| cx.new(KeepAwake::new).into());
}

/// The tile, lit while the stages hold off idle.
struct KeepAwake {
    on: bool,
    _followed: Vec<Subscription>,
}

impl KeepAwake {
    fn new(cx: &mut Context<Self>) -> Self {
        // Lit on every screen's drawer when one turns it on.
        let mut followed = Stage::observe_panels(cx, |tile: &mut Self, cx| {
            let on = Stage::is_kept_awake(cx);
            if on != tile.on {
                tile.on = on;
                cx.notify();
            }
        })
        .into_iter()
        .collect::<Vec<_>>();
        // The feature went: idle again.
        followed.push(cx.on_release(|_, cx| {
            cx.defer(|cx| {
                if !tile_is_wanted(cx) {
                    Stage::keep_awake(false, cx);
                }
            });
        }));
        Self { on: Stage::is_kept_awake(cx), _followed: followed }
    }
}

/// Whether the tile still has its slot (its screen went, not the feature).
fn tile_is_wanted(cx: &App) -> bool {
    Stage::slots(cx).is_some_and(|slots| slots.read(cx).for_host(HOST, cx).iter().any(|view| view == TILE))
}

impl Render for KeepAwake {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let on = self.on;
        Tile::new("keep-awake", Glyph::CUP, "Keep awake").on(on).on_toggle(move |_, _, cx| Stage::keep_awake(!on, cx))
    }
}
