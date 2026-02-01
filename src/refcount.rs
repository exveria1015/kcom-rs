// refcount.rs
//
// Shared refcount helpers with optional hardening.

use core::sync::atomic::{AtomicU32, Ordering};

#[cfg(feature = "refcount-hardening")]
const MAX_REFCOUNT: u32 = i32::MAX as u32;

#[cfg(feature = "refcount-hardening")]
#[cold]
#[inline(never)]
fn refcount_violation() -> ! {
    #[cfg(debug_assertions)]
    crate::trace::report_error(file!(), line!(), crate::STATUS_UNSUCCESSFUL);
    unsafe { core::intrinsics::abort() }
}

#[cfg(not(feature = "refcount-hardening"))]
#[inline]
pub(crate) fn add(ref_count: &AtomicU32) -> u32 {
    ref_count.fetch_add(1, Ordering::Relaxed) + 1
}

#[cfg(feature = "refcount-hardening")]
#[inline]
pub(crate) fn add(ref_count: &AtomicU32) -> u32 {
    match ref_count.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |curr| {
        if curr >= MAX_REFCOUNT {
            None
        } else {
            Some(curr + 1)
        }
    }) {
        Ok(prev) => prev + 1,
        Err(_) => refcount_violation(),
    }
}

#[cfg(not(feature = "refcount-hardening"))]
#[inline]
pub(crate) fn sub(ref_count: &AtomicU32) -> u32 {
    ref_count.fetch_sub(1, Ordering::Release) - 1
}

#[cfg(feature = "refcount-hardening")]
#[inline]
pub(crate) fn sub(ref_count: &AtomicU32) -> u32 {
    match ref_count.fetch_update(Ordering::Release, Ordering::Relaxed, |curr| {
        if curr == 0 {
            None
        } else {
            Some(curr - 1)
        }
    }) {
        Ok(prev) => prev - 1,
        Err(_) => refcount_violation(),
    }
}
