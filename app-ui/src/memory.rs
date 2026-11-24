#[cfg(all(target_os = "linux", target_env = "gnu"))]
// Each carousel open kept 30 to 100 MB (592 MB after use against 144 MB at start); 1 MB sits above a frame's
// allocations and below every picture: https://linear.app/imperaluna/issue/IMP-462
const OWN_MAPPING_FROM: std::ffi::c_int = 1024 * 1024;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
// glibc raises the mmap threshold after each mapped block is freed, so later pictures stay in arenas that keep the pages:
// https://man7.org/linux/man-pages/man3/mallopt.3.html
const M_MMAP_THRESHOLD: std::ffi::c_int = -3;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
unsafe extern "C" {
    fn mallopt(param: std::ffi::c_int, value: std::ffi::c_int) -> std::ffi::c_int;
}

pub fn give_back_large_blocks() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: mallopt takes plain integers and only changes malloc's
    // settings; no other thread exists yet.
    if unsafe { mallopt(M_MMAP_THRESHOLD, OWN_MAPPING_FROM) } == 0 {
        eprintln!("WARN nova-ui: malloc kept its own mapping threshold; freed pictures may stay in memory");
    }
}
