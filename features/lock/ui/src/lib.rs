mod lock;
mod pam;
mod prompt;
mod surface;
mod tokens;

use gpui::App;
use lock::LockScreen;

pub fn register(cx: &mut App) {
    nova_stage::overlay("lock.screen", cx, |cx| Box::new(LockScreen::get(cx)));
}
