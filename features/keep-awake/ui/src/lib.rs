use gpui::{App, AppContext, Context, IntoElement, Render, Subscription, Window};
use nova_look::{Glyph, Tile};
use nova_stage::Stage;

const TILE: &str = "keep-awake.tile";
const HOST: &str = "quick-settings.tiles";

pub fn register(cx: &mut App) {
    nova_stage::part(TILE, cx, |_, cx| cx.new(KeepAwake::new).into());
}

struct KeepAwake {
    on: bool,
    _followed: Vec<Subscription>,
}

impl KeepAwake {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut followed = Stage::observe_panels(cx, |tile: &mut Self, cx| {
            let on = Stage::is_kept_awake(cx);
            if on != tile.on {
                tile.on = on;
                cx.notify();
            }
        })
        .into_iter()
        .collect::<Vec<_>>();
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

fn tile_is_wanted(cx: &App) -> bool {
    Stage::slots(cx).is_some_and(|slots| slots.read(cx).for_host(HOST, cx).iter().any(|view| view == TILE))
}

impl Render for KeepAwake {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let on = self.on;
        Tile::new("keep-awake", Glyph::CUP, "Keep awake").on(on).on_toggle(move |_, _, cx| Stage::keep_awake(!on, cx))
    }
}
