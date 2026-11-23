//! How nova-ui's heap gives memory back (IMP-462).
//!
//! glibc's malloc serves a large block from its own mapping, which goes
//! back to the system when freed, but after each such free it raises that
//! threshold to the freed block's size (up to 32 MB). Pictures decoded on
//! worker threads (the wallpaper carousel's, the screenshot picker's frozen
//! screens) then land in per-thread arenas, which keep the pages once
//! freed: on RyzenShine each carousel open added 30 to 100 MB that never
//! came back (592 MB after use, against 144 MB right after start). A fixed
//! threshold keeps every block from 1 MB up in its own mapping, so it is
//! returned the moment it is freed.

/// Blocks from this size up are mapped on their own and unmapped on free:
/// above what a frame allocates over and over (each an mmap and munmap),
/// below every picture.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const OWN_MAPPING_FROM: std::ffi::c_int = 1024 * 1024;

/// glibc's `M_MMAP_THRESHOLD` (malloc.h).
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const M_MMAP_THRESHOLD: std::ffi::c_int = -3;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
unsafe extern "C" {
    fn mallopt(param: std::ffi::c_int, value: std::ffi::c_int) -> std::ffi::c_int;
}

/// Fixes malloc's mapping threshold. Called first thing in `main`, before
/// any thread starts.
pub fn give_back_large_blocks() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: mallopt takes plain integers and only changes malloc's
    // settings; no other thread exists yet.
    if unsafe { mallopt(M_MMAP_THRESHOLD, OWN_MAPPING_FROM) } == 0 {
        eprintln!("WARN nova-ui: malloc kept its own mapping threshold; freed pictures may stay in memory");
    }
}
