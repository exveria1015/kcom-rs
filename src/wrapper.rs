// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use alloc::boxed::Box;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::iunknown::{GUID, IUnknownVtbl, IID_IUNKNOWN, NTSTATUS, STATUS_NOINTERFACE, STATUS_SUCCESS};
use crate::traits::{ComImpl, InterfaceVtable};

#[repr(C)]
pub struct ComObject<T, I>
where
    T: ComImpl<I>,
    I: InterfaceVtable,
{
    vtable: &'static I,
    ref_count: AtomicU32,
    outer_unknown: Option<*mut IUnknownVtbl>,
    pub inner: T,
}

impl<T, I> ComObject<T, I>
where
    T: ComImpl<I>,
    I: InterfaceVtable,
{
    #[inline]
    pub fn new(inner: T) -> *mut c_void {
        let obj = Box::new(Self {
            vtable: T::VTABLE,
            ref_count: AtomicU32::new(1),
            outer_unknown: None,
            inner,
        });
        Box::into_raw(obj) as *mut c_void
    }

    #[inline]
    pub fn new_aggregated(inner: T, outer_unknown: *mut IUnknownVtbl) -> *mut c_void {
        let obj = Box::new(Self {
            vtable: T::VTABLE,
            ref_count: AtomicU32::new(1),
            outer_unknown: Some(outer_unknown),
            inner,
        });
        Box::into_raw(obj) as *mut c_void
    }

    #[inline(always)]
    pub unsafe fn from_ptr<'a>(ptr: *mut c_void) -> &'a Self {
        unsafe { &*(ptr as *const Self) }
    }

    #[allow(non_snake_case)]
    pub unsafe extern "system" fn shim_add_ref(this: *mut c_void) -> u32 {
        let wrapper = unsafe { Self::from_ptr(this) };
        if let Some(outer) = wrapper.outer_unknown {
            if !outer.is_null() {
                return unsafe { ((*outer).AddRef)(outer as *mut c_void) };
            }
        }
        wrapper.ref_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    #[allow(non_snake_case)]
    pub unsafe extern "system" fn shim_release(this: *mut c_void) -> u32 {
        let wrapper = unsafe { &*(this as *const Self) };
        if let Some(outer) = wrapper.outer_unknown {
            if !outer.is_null() {
                return unsafe { ((*outer).Release)(outer as *mut c_void) };
            }
        }
        let count = wrapper.ref_count.fetch_sub(1, Ordering::Release) - 1;

        if count == 0 {
            core::sync::atomic::fence(Ordering::Acquire);
            unsafe {
                let _ = Box::from_raw(this as *mut Self);
            }
        }

        count
    }

    #[allow(non_snake_case)]
    pub unsafe extern "system" fn shim_query_interface(
        this: *mut c_void,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> NTSTATUS {
        let wrapper = unsafe { Self::from_ptr(this) };
        if let Some(outer) = wrapper.outer_unknown {
            if !outer.is_null() {
                return unsafe { ((*outer).QueryInterface)(outer as *mut c_void, riid, ppv) };
            }
        }

        if ppv.is_null() || riid.is_null() {
            return STATUS_NOINTERFACE;
        }

        let riid = unsafe { &*riid };

        if *riid == IID_IUNKNOWN {
            unsafe { Self::shim_add_ref(this) };
            unsafe { *ppv = this };
            return STATUS_SUCCESS;
        }

        if let Some(ptr) = wrapper.inner.query_interface(riid) {
            unsafe { ((*(ptr as *mut IUnknownVtbl)).AddRef)(ptr) };
            unsafe { *ppv = ptr };
            return STATUS_SUCCESS;
        }

        unsafe { *ppv = core::ptr::null_mut() };
        STATUS_NOINTERFACE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use core::sync::atomic::{AtomicU32, Ordering};

    static DROP_COUNT: AtomicU32 = AtomicU32::new(0);

    struct Dummy;

    impl Drop for Dummy {
        fn drop(&mut self) {
            DROP_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn add_ref_release_drops() {
        DROP_COUNT.store(0, Ordering::Relaxed);

        let ptr = ComObject::<Dummy, IUnknownVtbl>::new(Dummy);

        unsafe {
            assert_eq!(ComObject::<Dummy, IUnknownVtbl>::shim_add_ref(ptr), 2);
            assert_eq!(ComObject::<Dummy, IUnknownVtbl>::shim_release(ptr), 1);
            assert_eq!(ComObject::<Dummy, IUnknownVtbl>::shim_release(ptr), 0);
        }

        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn query_interface_iunknown_returns_self() {
        DROP_COUNT.store(0, Ordering::Relaxed);

        let ptr = ComObject::<Dummy, IUnknownVtbl>::new(Dummy);
        let mut out = core::ptr::null_mut();

        let status = unsafe {
            ComObject::<Dummy, IUnknownVtbl>::shim_query_interface(
                ptr,
                &IID_IUNKNOWN,
                &mut out,
            )
        };

        assert_eq!(status, STATUS_SUCCESS);
        assert_eq!(out, ptr);

        unsafe {
            assert_eq!(ComObject::<Dummy, IUnknownVtbl>::shim_release(ptr), 1);
            assert_eq!(ComObject::<Dummy, IUnknownVtbl>::shim_release(ptr), 0);
        }

        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 1);
    }
}
