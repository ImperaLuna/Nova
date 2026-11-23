use std::ffi::OsString;
use std::path::Path;

pub use crate::features::{read_selection, states as feature_states};
pub use crate::supervisor::GPUI_UI;
pub use nova_host::os_release_field;
pub use nova_process::child::output_within;
pub use nova_theme::{DEFAULT_THEME_ID, default_dir as default_themes_dir};

pub fn session_path() -> String {
    crate::session_path()
}

pub fn read_config(path: &Path, themes_dir: &Path) -> Result<Vec<String>, String> {
    let config = crate::config::read(path)?;
    Ok(nova_theme::resolve(themes_dir, &config.theme).1)
}

pub fn compositor(env: impl Fn(&str) -> Option<OsString>) -> Option<&'static str> {
    nova_compositor::detect(env).map(|backend| backend.name())
}

pub fn restores_session_lock(env: impl Fn(&str) -> Option<OsString>) -> Option<Result<bool, String>> {
    nova_compositor::detect(env).map(|backend| backend.restores_session_lock())
}
