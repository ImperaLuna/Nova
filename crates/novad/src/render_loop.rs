//! Qt's render loop for the UI. On NVIDIA's driver the threaded loop gives
//! each window a render thread and graphics context of its own, which cost
//! NovaUI about 30 MB Pss on RyzenShine (IMP-159); there the UI gets the
//! basic loop, which draws every window on one thread. Elsewhere (Mesa,
//! nouveau, no GPU) Qt picks its own.

use nova_features::gpus;
use std::ffi::OsString;
use std::path::Path;

pub const VAR: &str = "QSG_RENDER_LOOP";
const ON_NVIDIA: &str = "basic";

/// The render loop to start the UI with on the machine below `root`: the
/// basic one while NVIDIA's driver is loaded, unless the session (`env`)
/// picks one itself.
pub fn for_ui(root: &Path, env: impl Fn(&str) -> Option<OsString>) -> Option<&'static str> {
    (env(VAR).is_none() && gpus::nvidia_driver(root)).then_some(ON_NVIDIA)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_features::gpus::NVIDIA_MODULE;
    use nova_test_support::TempDir;

    #[test]
    fn only_nvidias_driver_gets_the_basic_loop_unless_the_session_picks() {
        let no_env = |_: &str| None;
        let nvidia = TempDir::new("render-loop-nvidia");
        std::fs::create_dir_all(nvidia.join(NVIDIA_MODULE)).unwrap();
        assert_eq!(for_ui(&nvidia, no_env), Some(ON_NVIDIA));
        let set = |name: &str| (name == VAR).then(|| OsString::from("threaded"));
        assert_eq!(for_ui(&nvidia, set), None);
        // Mesa, nouveau or no GPU at all: no NVIDIA module.
        let other = TempDir::new("render-loop-other");
        assert_eq!(for_ui(&other, no_env), None);
    }
}
