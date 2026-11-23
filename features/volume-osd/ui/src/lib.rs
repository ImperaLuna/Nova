mod capsule;
mod logic;
mod motion;
mod shader;
mod tokens;
mod window;

use gpui::App;

pub fn register(cx: &mut App) {
    nova_stage::overlay("volume-osd.capsule", cx, |cx| Box::new(capsule::Capsule::start(cx)));
}
