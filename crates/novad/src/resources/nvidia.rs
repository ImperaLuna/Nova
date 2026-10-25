//! How busy an NVIDIA GPU is, from NVIDIA's management library (NVML,
//! `libnvidia-ml.so.1`, which comes with the driver). The proprietary driver
//! reports nothing in sysfs. The library is loaded once, if it's there, and
//! NVML is started only while resources are watched (starting it wakes the
//! driver) and shut down when watching stops.
//!
//! It's the library `nvidia-smi` reads too, without running that binary: on
//! NixOS with driver 595.104.02 and glibc 2.44, `nvidia-smi` crashed in the
//! dynamic loader before doing anything (IMP-104). Where the library can't
//! be loaded (the static musl release can't load any), `nvidia_smi` runs
//! `nvidia-smi` instead.

use std::ffi::{CString, c_char, c_int, c_uint, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// The library's name, found by the dynamic loader's search path.
const LIBRARY: &str = "libnvidia-ml.so.1";
/// Where NixOS puts the driver's libraries, outside any search path.
const NIXOS_DRIVER_DIR: &str = "/run/opengl-driver/lib";
/// NVML_SUCCESS.
const SUCCESS: c_int = 0;

/// Where the library is looked for, in order.
pub fn library_paths() -> Vec<PathBuf> {
    vec![PathBuf::from(LIBRARY), Path::new(NIXOS_DRIVER_DIR).join(LIBRARY)]
}

type Device = *mut c_void;

/// nvmlUtilization_t: percent of time busy over the last sample period.
#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: c_uint,
    memory: c_uint,
}

/// The NVML functions used, from a library that stays loaded.
#[derive(Clone, Copy)]
pub struct Library {
    init: unsafe extern "C" fn() -> c_int,
    shutdown: unsafe extern "C" fn() -> c_int,
    device_by_index: unsafe extern "C" fn(c_uint, *mut Device) -> c_int,
    utilization: unsafe extern "C" fn(Device, *mut Utilization) -> c_int,
}

impl Library {
    /// The first of `paths` that loads and has every function, if any.
    pub fn open(paths: &[PathBuf]) -> Option<Self> {
        paths.iter().find_map(|path| Self::open_one(path))
    }

    fn open_one(path: &Path) -> Option<Self> {
        let name = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: a valid C string. The handle is never closed, so the
        // functions below stay valid for the process's life.
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return None;
        }
        let symbol = |name: &[u8]| {
            // SAFETY: `name` is NUL-terminated and `handle` is open.
            let found = unsafe { libc::dlsym(handle, name.as_ptr().cast::<c_char>()) };
            (!found.is_null()).then_some(found)
        };
        let (init, shutdown, device_by_index, utilization) = (
            symbol(b"nvmlInit_v2\0")?,
            symbol(b"nvmlShutdown\0")?,
            symbol(b"nvmlDeviceGetHandleByIndex_v2\0")?,
            symbol(b"nvmlDeviceGetUtilizationRates\0")?,
        );
        // SAFETY: NVML's documented signatures for these names (nvml.h).
        unsafe {
            Some(Self {
                init: std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> c_int>(init),
                shutdown: std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> c_int>(shutdown),
                device_by_index: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(c_uint, *mut Device) -> c_int>(
                    device_by_index,
                ),
                utilization: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(Device, *mut Utilization) -> c_int>(
                    utilization,
                ),
            })
        }
    }

    /// Starts NVML and finds the first GPU.
    pub fn start(&self) -> Result<Nvml, String> {
        // SAFETY: no arguments; NVML counts starts and shutdowns.
        let status = unsafe { (self.init)() };
        if status != SUCCESS {
            return Err(format!("nvmlInit_v2 returned {status}"));
        }
        let mut device: Device = std::ptr::null_mut();
        // SAFETY: NVML is started and `device` is a valid out pointer.
        let status = unsafe { (self.device_by_index)(0, &raw mut device) };
        if status != SUCCESS {
            // SAFETY: balances the start above.
            unsafe { (self.shutdown)() };
            return Err(format!("nvmlDeviceGetHandleByIndex_v2 returned {status}"));
        }
        Ok(Nvml { library: *self, device })
    }
}

/// NVML while it's started, and the first GPU. Shut down when dropped.
pub struct Nvml {
    library: Library,
    device: Device,
}

// SAFETY: NVML's calls are thread-safe, and the device handle is only an
// id NVML hands back to itself.
unsafe impl Send for Nvml {}

impl Nvml {
    /// How busy the GPU is now, in percent.
    pub fn busy(&self) -> Option<f32> {
        let mut utilization = Utilization::default();
        // SAFETY: a device from this started NVML, and a valid out pointer.
        let status = unsafe { (self.library.utilization)(self.device, &raw mut utilization) };
        (status == SUCCESS && utilization.gpu <= 100).then_some(utilization.gpu as f32)
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: balances the start that made it.
        unsafe { (self.library.shutdown)() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_library_means_no_reading() {
        let missing = std::env::temp_dir().join("novad-no-such-dir").join(LIBRARY);
        assert!(Library::open(&[missing]).is_none());
        assert!(Library::open(&[]).is_none());
    }

    /// Only where NVIDIA's driver is (the desktop, not CI's build sandbox).
    #[test]
    fn reads_the_gpu_where_there_is_one() {
        let Some(library) = Library::open(&library_paths()) else { return };
        let nvml = library.start().unwrap();
        let busy = nvml.busy().unwrap();
        assert!((0.0..=100.0).contains(&busy));
        drop(nvml);
        // Started again after a shutdown, as each watch does.
        assert!(library.start().unwrap().busy().is_some());
    }
}
