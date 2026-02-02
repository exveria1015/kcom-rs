use core::ffi::c_void;

/// Pointer wrapper used by async guards to avoid exposing provenance casts in strict builds.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct GuardPtr(
    #[cfg(feature = "strict-provenance")] *mut c_void,
    #[cfg(not(feature = "strict-provenance"))] usize,
);

unsafe impl Send for GuardPtr {}
unsafe impl Sync for GuardPtr {}

impl GuardPtr {
    #[inline]
    pub fn new(ptr: *mut c_void) -> Self {
        #[cfg(feature = "strict-provenance")]
        {
            Self(ptr)
        }
        #[cfg(not(feature = "strict-provenance"))]
        {
            Self(ptr as usize)
        }
    }

    #[inline]
    pub fn as_ptr(self) -> *mut c_void {
        #[cfg(feature = "strict-provenance")]
        {
            self.0
        }
        #[cfg(not(feature = "strict-provenance"))]
        {
            self.0 as *mut c_void
        }
    }
}
