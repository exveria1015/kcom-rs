// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::ffi::c_void;
use core::marker::PhantomData;
use core::ptr::NonNull;

use crate::iunknown::IUnknownVtbl;

/// Marker trait for types that are valid COM interfaces.
///
/// # Safety
/// Implementors guarantee that:
/// 1. The type is `Sized` (no fat pointers allowed).
/// 2. The type is `#[repr(C)]` or `#[repr(transparent)]` and has the same memory layout
///    as a COM interface pointer (the first field is a pointer to the vtable).
/// 3. The vtable begins with the `IUnknown` methods.
pub unsafe trait ComInterface: Sized {}

/// Reference-counted COM interface pointer.
///
/// # Safety
/// The pointer must be a valid COM interface pointer whose vtable begins with
/// `IUnknown` methods.
///
/// # Thread Safety
/// This type does not implement `Send` or `Sync` by default because many COM
/// interfaces are thread-affine. If you use `ComRc` with interfaces that are
/// explicitly free-threaded in your environment, wrap or newtype it and add a
/// documented `unsafe impl Send/Sync` for that specific case.
pub struct ComRc<T: ComInterface> {
    ptr: NonNull<T>,
    _phantom: PhantomData<T>,
}

impl<T: ComInterface> ComRc<T> {
    /// Takes ownership of a raw COM pointer without calling `AddRef`.
    ///
    /// # Safety
    /// `ptr` must be a valid COM interface pointer.
    pub unsafe fn from_raw(ptr: *mut T) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self {
            ptr,
            _phantom: PhantomData,
        })
    }

    /// Takes ownership of a raw COM pointer and calls `AddRef` first.
    ///
    /// # Safety
    /// `ptr` must be a valid COM interface pointer.
    pub unsafe fn from_raw_addref(ptr: *mut T) -> Option<Self> {
        if ptr.is_null() {
            return None;
        }
        // SAFETY: caller guarantees `ptr` is a valid COM interface pointer.
        unsafe { add_ref(ptr) };
        // SAFETY: caller guarantees `ptr` is a valid COM interface pointer.
        unsafe { Self::from_raw(ptr) }
    }

    #[inline]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }

    #[inline]
    pub fn into_raw(self) -> *mut T {
        let ptr = self.ptr.as_ptr();
        core::mem::forget(self);
        ptr
    }
}

impl<T: ComInterface> core::ops::Deref for ComRc<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.as_ref() }
    }
}

impl<T: ComInterface> Clone for ComRc<T> {
    fn clone(&self) -> Self {
        unsafe { add_ref(self.ptr.as_ptr()) };
        Self {
            ptr: self.ptr,
            _phantom: PhantomData,
        }
    }
}

impl<T: ComInterface> Drop for ComRc<T> {
    fn drop(&mut self) {
        unsafe { release(self.ptr.as_ptr()) };
    }
}

unsafe fn add_ref<T: ComInterface>(ptr: *mut T) -> u32 {
    let vtbl = unsafe { *(ptr as *mut *mut IUnknownVtbl) };
    unsafe { ((*vtbl).AddRef)(ptr as *mut c_void) }
}

unsafe fn release<T: ComInterface>(ptr: *mut T) -> u32 {
    let vtbl = unsafe { *(ptr as *mut *mut IUnknownVtbl) };
    unsafe { ((*vtbl).Release)(ptr as *mut c_void) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrapper::ComObject;
    use core::sync::atomic::{AtomicU32, Ordering};

    static DROP_COUNT: AtomicU32 = AtomicU32::new(0);

    #[repr(C)]
    #[allow(non_snake_case)]
    struct IUnknownRaw {
        #[allow(non_snake_case)]
        lpVtbl: *mut IUnknownVtbl,
    }

    unsafe impl ComInterface for IUnknownRaw {}

    struct Dummy;

    impl Drop for Dummy {
        fn drop(&mut self) {
            DROP_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn from_raw_addref_balances_release() {
        DROP_COUNT.store(0, Ordering::Relaxed);
        let raw = ComObject::<Dummy, IUnknownVtbl>::new(Dummy);

        let com = unsafe { ComRc::<IUnknownRaw>::from_raw_addref(raw as *mut IUnknownRaw).unwrap() };
        drop(com);

        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 0);

        unsafe {
            assert_eq!(ComObject::<Dummy, IUnknownVtbl>::shim_release(raw), 0);
        }

        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn from_raw_consumes_reference() {
        DROP_COUNT.store(0, Ordering::Relaxed);
        let raw = ComObject::<Dummy, IUnknownVtbl>::new(Dummy);

        let com = unsafe { ComRc::<IUnknownRaw>::from_raw(raw as *mut IUnknownRaw).unwrap() };
        drop(com);

        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn clone_adds_reference() {
        DROP_COUNT.store(0, Ordering::Relaxed);
        let raw = ComObject::<Dummy, IUnknownVtbl>::new(Dummy);

        let com = unsafe { ComRc::<IUnknownRaw>::from_raw_addref(raw as *mut IUnknownRaw).unwrap() };
        let com_clone = com.clone();
        drop(com);
        drop(com_clone);

        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 0);

        unsafe {
            assert_eq!(ComObject::<Dummy, IUnknownVtbl>::shim_release(raw), 0);
        }

        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 1);
    }
}
