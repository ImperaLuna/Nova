pub mod desktop_entry;
mod index;
pub mod launch;
mod program;
pub mod scan;

pub use index::{AppIndex, Sources, State};
pub use launch::open;
pub use program::{XEMBED_TRAY_BRIDGE, program_name};

pub const APP_ICON_MAX_CHARS: usize = 512;

pub fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}
