use core::ffi::c_void;

/// Pointer wrapper used by async guards to avoid exposing provenance casts in strict builds.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct GuardPtr(
    #[cfg(any(feature = "strict-provenance", kcom_strict_provenance, miri))] *mut c_void,
    #[cfg(not(any(feature = "strict-provenance", kcom_strict_provenance, miri)))] usize,
);

unsafe impl Send for GuardPtr {}
unsafe impl Sync for GuardPtr {}

impl GuardPtr {
    #[inline]
    pub fn new(ptr: *mut c_void) -> Self {
        #[cfg(any(feature = "strict-provenance", kcom_strict_provenance, miri))]
        {
            Self(ptr)
        }
        #[cfg(not(any(feature = "strict-provenance", kcom_strict_provenance, miri)))]
        {
            Self(ptr as usize)
        }
    }

    #[inline]
    pub fn as_ptr(self) -> *mut c_void {
        #[cfg(any(feature = "strict-provenance", kcom_strict_provenance, miri))]
        {
            self.0
        }
        #[cfg(not(any(feature = "strict-provenance", kcom_strict_provenance, miri)))]
        {
            self.0 as *mut c_void
        }
    }
}
