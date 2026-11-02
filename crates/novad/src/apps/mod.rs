//! The apps installed, as their desktop entries describe them: reading the
//! entries (`desktop_entry`), finding them in the XDG data folders (`scan`),
//! the index of them all that follows installs (`index`), and telling which
//! app a running program is (`program`). The launcher, tray, notifications
//! and media all name apps from the index.

pub mod desktop_entry;
mod index;
mod program;
pub mod scan;

pub use index::{AppIndex, Sources, State};
pub use program::program_name;

/// `XDG_CURRENT_DESKTOP`'s names, like `niri`, which entries show in or not.
pub fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}
