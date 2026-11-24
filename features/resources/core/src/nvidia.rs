use std::ffi::{CString, c_char, c_int, c_uint, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

// NVML instead of running nvidia-smi: with driver 595.104.02 and glibc 2.44 nvidia-smi crashed in the dynamic loader:
// https://linear.app/imperaluna/issue/IMP-104
const LIBRARY: &str = "libnvidia-ml.so.1";
const NIXOS_DRIVER_DIR: &str = "/run/opengl-driver/lib";
const SUCCESS: c_int = 0;

pub fn library_paths() -> Vec<PathBuf> {
    vec![PathBuf::from(LIBRARY), Path::new(NIXOS_DRIVER_DIR).join(LIBRARY)]
}

type Device = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: c_uint,
    memory: c_uint,
}

// Starting NVML loads libcuda (about 19 MB), so the library is loaded only while resources are watched:
// https://linear.app/imperaluna/issue/IMP-151
pub struct Library {
    handle: *mut c_void,
    init: unsafe extern "C" fn() -> c_int,
    shutdown: unsafe extern "C" fn() -> c_int,
    device_by_index: unsafe extern "C" fn(c_uint, *mut Device) -> c_int,
    utilization: unsafe extern "C" fn(Device, *mut Utilization) -> c_int,
}

impl Library {
    pub fn open(paths: &[PathBuf]) -> Option<Self> {
        paths.iter().find_map(|path| Self::open_one(path))
    }

    fn open_one(path: &Path) -> Option<Self> {
        let name = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: a valid C string. The handle is closed only when the
        // `Library` holding the functions below is dropped.
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return None;
        }
        let symbol = |name: &[u8]| {
            // SAFETY: `name` is NUL-terminated and `handle` is open.
            let found = unsafe { libc::dlsym(handle, name.as_ptr().cast::<c_char>()) };
            (!found.is_null()).then_some(found)
        };
        let found = (
            symbol(b"nvmlInit_v2\0"),
            symbol(b"nvmlShutdown\0"),
            symbol(b"nvmlDeviceGetHandleByIndex_v2\0"),
            symbol(b"nvmlDeviceGetUtilizationRates\0"),
        );
        let (Some(init), Some(shutdown), Some(device_by_index), Some(utilization)) = found else {
            // SAFETY: opened above, and nothing from it is kept.
            unsafe { libc::dlclose(handle) };
            return None;
        };
        // SAFETY: NVML's documented signatures for these names (nvml.h).
        unsafe {
            Some(Self {
                handle,
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

    pub fn start(self) -> Result<Nvml, String> {
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
        Ok(Nvml { library: self, device })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: opened in `open_one`; nothing outlives the `Library`.
        unsafe { libc::dlclose(self.handle) };
    }
}

pub struct Nvml {
    library: Library,
    device: Device,
}

// SAFETY: NVML's calls are thread-safe, the device handle is only an id
// NVML hands back to itself, and dlclose may run on any thread.
unsafe impl Send for Nvml {}
// SAFETY: as for `Nvml`: the handle is only passed back to dlsym and dlclose.
unsafe impl Send for Library {}

impl Nvml {
    pub fn busy(&self) -> Option<f32> {
        let mut utilization = Utilization::default();
        // SAFETY: a device from this started NVML, and a valid out pointer.
        let status = unsafe { (self.library.utilization)(self.device, &raw mut utilization) };
        (status == SUCCESS && utilization.gpu <= 100).then_some(utilization.gpu as f32)
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: balances the start that made it, before `library` unloads.
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

    #[test]
    fn reads_the_gpu_where_there_is_one() {
        let Some(library) = Library::open(&library_paths()) else { return };
        let nvml = library.start().unwrap();
        let busy = nvml.busy().unwrap();
        assert!((0.0..=100.0).contains(&busy));
        drop(nvml);
        let library = Library::open(&library_paths()).unwrap();
        assert!(library.start().unwrap().busy().is_some());
    }
}
