//! What `nova doctor` and `nova setup` ask of novad's own readers, so they
//! and the daemon never disagree about a config file, a feature's binaries,
//! the Quickshell they'd run or the compositor they'd follow. The CLI runs
//! its own short commands (systemctl) through novad's timed wait too.

use std::ffi::OsString;
use std::path::Path;

pub use crate::child::output_within;
pub use crate::features::{read_selection, states as feature_states};
pub use crate::supervisor::{UI_PROCESS_NAME, default_quickshell, read_quickshell_version};
pub use crate::theme::{DEFAULT_THEME_ID, default_dir as default_themes_dir};

/// The `PATH` novad looks for binaries in (feature checks, Quickshell).
pub fn session_path() -> String {
    crate::session_path()
}

/// Reads `config.toml` at `path` as novad does: an error when it doesn't
/// parse, else the problems with the theme it names (novad falls back to
/// the default theme for those). A missing file is fine.
pub fn read_config(path: &Path, themes_dir: &Path) -> Result<Vec<String>, String> {
    let config = crate::config::read(path)?;
    Ok(crate::theme::resolve(themes_dir, &config.theme).1)
}

/// The compositor novad would follow with this environment, by name.
pub fn compositor(env: impl Fn(&str) -> Option<OsString>) -> Option<&'static str> {
    crate::compositor::detect(env).map(|backend| backend.name())
}
