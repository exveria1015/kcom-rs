// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::alloc::Layout;
use core::ffi::c_void;
use core::mem::ManuallyDrop;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::allocator::{Allocator, GlobalAllocator};
use crate::iunknown::{
    GUID, IUnknownVtbl, IID_IUNKNOWN, NTSTATUS, STATUS_INSUFFICIENT_RESOURCES, STATUS_NOINTERFACE,
    STATUS_SUCCESS,
};
use crate::smart_ptr::{ComInterface, ComRc};
use crate::traits::{ComImpl, ComInterfaceInfo, InterfaceVtable};

#[repr(C)]
struct NonDelegatingIUnknown<T, I, A>
where
    T: ComImpl<I>,
    I: InterfaceVtable,
    A: Allocator + Send + Sync,
{
    vtable: &'static IUnknownVtbl,
    parent: *mut ComObject<T, I, A>,
}

impl<T, P, S> ComObject2<T, P, S, GlobalAllocator>
where
    T: ComImpl<P> + ComImpl<S>,
    P: InterfaceVtable,
    S: InterfaceVtable,
{
    #[inline]
    pub fn new(inner: T) -> Result<*mut c_void, NTSTATUS> {
        Self::new_in(inner, GlobalAllocator)
    }

    #[inline]
    pub fn try_new(inner: T) -> Option<*mut c_void> {
        Self::try_new_in(inner, GlobalAllocator)
    }

    /// Creates a COM object and returns a smart pointer that owns the initial reference.
    #[inline]
    pub fn new_rc<R>(inner: T) -> Result<ComRc<R>, NTSTATUS>
    where
        R: ComInterface + ComInterfaceInfo<Vtable = P>,
    {
        Self::new_rc_in(inner, GlobalAllocator)
    }

    /// Creates a COM object and returns a smart pointer that owns the initial reference.
    #[inline]
    pub fn try_new_rc<R>(inner: T) -> Option<ComRc<R>>
    where
        R: ComInterface + ComInterfaceInfo<Vtable = P>,
    {
        Self::try_new_rc_in(inner, GlobalAllocator)
    }
}

#[repr(C)]
struct InterfaceEntry<I, O>
where
    I: InterfaceVtable,
{
    vtable: &'static I,
    parent: *mut O,
}

#[repr(C)]
struct NonDelegatingIUnknown2<T, P, S, A>
where
    T: ComImpl<P> + ComImpl<S>,
    P: InterfaceVtable,
    S: InterfaceVtable,
    A: Allocator + Send + Sync,
{
    vtable: &'static IUnknownVtbl,
    parent: *mut ComObject2<T, P, S, A>,
}

#[repr(C)]
pub struct ComObject2<T, P, S, A = GlobalAllocator>
where
    T: ComImpl<P> + ComImpl<S>,
    P: InterfaceVtable,
    S: InterfaceVtable,
    A: Allocator + Send + Sync,
{
    vtable: &'static P,
    secondary: InterfaceEntry<S, ComObject2<T, P, S, A>>,
    non_delegating_unknown: NonDelegatingIUnknown2<T, P, S, A>,
    ref_count: AtomicU32,
    outer_unknown: Option<*mut IUnknownVtbl>,
    pub inner: T,
    alloc: ManuallyDrop<A>,
}

impl<T, P, S, A> ComObject2<T, P, S, A>
where
    T: ComImpl<P> + ComImpl<S>,
    P: InterfaceVtable,
    S: InterfaceVtable,
    A: Allocator + Send + Sync,
{
    const LAYOUT: Layout = Layout::new::<Self>();
    const NON_DELEGATING_VTABLE: IUnknownVtbl = IUnknownVtbl {
        QueryInterface: Self::shim_non_delegating_query_interface,
        AddRef: Self::shim_non_delegating_add_ref,
        Release: Self::shim_non_delegating_release,
    };

    #[inline]
    fn init_non_delegating_ptr(ptr: *mut Self) {
        unsafe {
            (*ptr).non_delegating_unknown.parent = ptr;
        }
    }

    #[inline]
    fn init_secondary_ptr(ptr: *mut Self) {
        unsafe {
            (*ptr).secondary.parent = ptr;
        }
    }

    #[inline]
    pub unsafe fn secondary_ptr(ptr: *mut Self) -> *mut c_void {
        unsafe { &mut (*ptr).secondary as *mut _ as *mut c_void }
    }

    #[inline(always)]
    /// # Safety
    /// `ptr` must be a valid pointer to a `ComObject2<T, P, S>` allocated by this crate.
    pub unsafe fn from_ptr<'a>(ptr: *mut c_void) -> &'a Self {
        unsafe { &*(ptr as *const Self) }
    }

    #[inline(always)]
    /// # Safety
    /// `ptr` must be a valid pointer to the secondary interface entry for this object.
    pub unsafe fn from_secondary_ptr<'a>(ptr: *mut c_void) -> &'a Self {
        let entry = ptr as *const InterfaceEntry<S, Self>;
        unsafe { &*(*entry).parent }
    }

    #[inline]
    pub fn new_rc_in<R>(inner: T, alloc: A) -> Result<ComRc<R>, NTSTATUS>
    where
        R: ComInterface + ComInterfaceInfo<Vtable = P>,
    {
        Self::try_new_rc_in(inner, alloc).ok_or(STATUS_INSUFFICIENT_RESOURCES)
    }

    #[inline]
    pub fn try_new_rc_in<R>(inner: T, alloc: A) -> Option<ComRc<R>>
    where
        R: ComInterface + ComInterfaceInfo<Vtable = P>,
    {
        let ptr = Self::try_new_in(inner, alloc)?;
        Some(unsafe { ComRc::from_raw_unchecked(ptr as *mut R) })
    }

    #[inline]
    pub fn new_in(inner: T, alloc: A) -> Result<*mut c_void, NTSTATUS> {
        Self::try_new_in(inner, alloc).ok_or(STATUS_INSUFFICIENT_RESOURCES)
    }

    #[inline]
    pub fn try_new_in(inner: T, alloc: A) -> Option<*mut c_void> {
        let ptr = unsafe { alloc.alloc(Self::LAYOUT) } as *mut Self;
        if ptr.is_null() {
            return None;
        }
        unsafe {
            ptr.write(Self {
                vtable: <T as ComImpl<P>>::VTABLE,
                secondary: InterfaceEntry {
                    vtable: <T as ComImpl<S>>::VTABLE,
                    parent: core::ptr::null_mut(),
                },
                non_delegating_unknown: NonDelegatingIUnknown2 {
                    vtable: &Self::NON_DELEGATING_VTABLE,
                    parent: core::ptr::null_mut(),
                },
                ref_count: AtomicU32::new(1),
                outer_unknown: None,
                inner,
                alloc: ManuallyDrop::new(alloc),
            });
            Self::init_non_delegating_ptr(ptr);
            Self::init_secondary_ptr(ptr);
            Some(ptr as *mut c_void)
        }
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid COM pointer created by `ComObject2` for `T`.
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
    /// # Safety
    /// `this` must be a valid COM pointer created by `ComObject2` for `T`.
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
            let ptr = this as *mut Self;
            let alloc = unsafe { core::ptr::read(&(*ptr).alloc) };
            let alloc = ManuallyDrop::into_inner(alloc);
            unsafe {
                core::ptr::drop_in_place(ptr);
                alloc.dealloc(ptr as *mut u8, Self::LAYOUT);
            }
            drop(alloc);
        }

        count
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid COM pointer created by `ComObject2` for `T`.
    /// `riid` and `ppv` must be valid, non-null pointers.
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

        if let Some(ptr) = <T as ComImpl<P>>::query_interface(&wrapper.inner, this, riid) {
            let vtbl = unsafe { *(ptr as *mut *mut IUnknownVtbl) };
            unsafe { ((*vtbl).AddRef)(ptr) };
            unsafe { *ppv = ptr };
            return STATUS_SUCCESS;
        }

        unsafe { *ppv = core::ptr::null_mut() };
        STATUS_NOINTERFACE
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid COM pointer created by `ComObject2` for `T`.
    pub unsafe extern "system" fn shim_add_ref_secondary(this: *mut c_void) -> u32 {
        let wrapper = unsafe { Self::from_secondary_ptr(this) };
        if let Some(outer) = wrapper.outer_unknown {
            if !outer.is_null() {
                return unsafe { ((*outer).AddRef)(outer as *mut c_void) };
            }
        }
        wrapper.ref_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid COM pointer created by `ComObject2` for `T`.
    pub unsafe extern "system" fn shim_release_secondary(this: *mut c_void) -> u32 {
        let wrapper = unsafe { Self::from_secondary_ptr(this) };
        let primary = wrapper as *const _ as *mut c_void;
        unsafe { Self::shim_release(primary) }
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid COM pointer created by `ComObject2` for `T`.
    pub unsafe extern "system" fn shim_query_interface_secondary(
        this: *mut c_void,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> NTSTATUS {
        let wrapper = unsafe { Self::from_secondary_ptr(this) };
        let primary = wrapper as *const _ as *mut c_void;
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
            unsafe { Self::shim_add_ref_secondary(this) };
            unsafe { *ppv = primary };
            return STATUS_SUCCESS;
        }

        if let Some(ptr) = <T as ComImpl<P>>::query_interface(&wrapper.inner, primary, riid) {
            let vtbl = unsafe { *(ptr as *mut *mut IUnknownVtbl) };
            unsafe { ((*vtbl).AddRef)(ptr) };
            unsafe { *ppv = ptr };
            return STATUS_SUCCESS;
        }

        unsafe { *ppv = core::ptr::null_mut() };
        STATUS_NOINTERFACE
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid non-delegating IUnknown pointer created by `ComObject2` for `T`.
    pub unsafe extern "system" fn shim_non_delegating_add_ref(this: *mut c_void) -> u32 {
        let wrapper = unsafe { Self::from_non_delegating(this) };
        wrapper.ref_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid non-delegating IUnknown pointer created by `ComObject2` for `T`.
    pub unsafe extern "system" fn shim_non_delegating_release(this: *mut c_void) -> u32 {
        let wrapper = unsafe { Self::from_non_delegating(this) };
        let count = wrapper.ref_count.fetch_sub(1, Ordering::Release) - 1;

        if count == 0 {
            core::sync::atomic::fence(Ordering::Acquire);
            let ptr = wrapper as *const Self as *mut Self;
            let alloc = unsafe { core::ptr::read(&(*ptr).alloc) };
            let alloc = ManuallyDrop::into_inner(alloc);
            unsafe {
                core::ptr::drop_in_place(ptr);
                alloc.dealloc(ptr as *mut u8, Self::LAYOUT);
            }
            drop(alloc);
        }

        count
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid non-delegating IUnknown pointer created by `ComObject2` for `T`.
    pub unsafe extern "system" fn shim_non_delegating_query_interface(
        this: *mut c_void,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> NTSTATUS {
        let wrapper = unsafe { Self::from_non_delegating(this) };
        if ppv.is_null() || riid.is_null() {
            return STATUS_NOINTERFACE;
        }

        let riid = unsafe { &*riid };

        if *riid == IID_IUNKNOWN {
            unsafe { Self::shim_non_delegating_add_ref(this) };
            unsafe { *ppv = this };
            return STATUS_SUCCESS;
        }

        let primary = wrapper as *const _ as *mut c_void;
        if let Some(ptr) = <T as ComImpl<P>>::query_interface(&wrapper.inner, primary, riid) {
            let vtbl = unsafe { *(ptr as *mut *mut IUnknownVtbl) };
            unsafe { ((*vtbl).AddRef)(ptr) };
            unsafe { *ppv = ptr };
            return STATUS_SUCCESS;
        }

        unsafe { *ppv = core::ptr::null_mut() };
        STATUS_NOINTERFACE
    }

    #[inline(always)]
    /// # Safety
    /// `ptr` must be a valid pointer to a non-delegating IUnknown created by this crate.
    unsafe fn from_non_delegating<'a>(ptr: *mut c_void) -> &'a Self {
        let unknown = unsafe { &*(ptr as *const NonDelegatingIUnknown2<T, P, S, A>) };
        unsafe { &*unknown.parent }
    }
}

#[repr(C)]
pub struct ComObject<T, I, A = GlobalAllocator>
where
    T: ComImpl<I>,
    I: InterfaceVtable,
    A: Allocator + Send + Sync,
{
    vtable: &'static I,
    non_delegating_unknown: NonDelegatingIUnknown<T, I, A>,
    ref_count: AtomicU32,
    outer_unknown: Option<*mut IUnknownVtbl>,
    pub inner: T,
    alloc: ManuallyDrop<A>,
}

impl<T, I, A> ComObject<T, I, A>
where
    T: ComImpl<I>,
    I: InterfaceVtable,
    A: Allocator + Send + Sync,
{
    const LAYOUT: Layout = Layout::new::<Self>();
    const NON_DELEGATING_VTABLE: IUnknownVtbl = IUnknownVtbl {
        QueryInterface: Self::shim_non_delegating_query_interface,
        AddRef: Self::shim_non_delegating_add_ref,
        Release: Self::shim_non_delegating_release,
    };

    #[inline]
    fn init_non_delegating_ptr(ptr: *mut Self) {
        unsafe {
            (*ptr).non_delegating_unknown.parent = ptr;
        }
    }

    /// Creates a COM object and returns a smart pointer that owns the initial reference.
    #[inline]
    pub fn new_rc_in<R>(inner: T, alloc: A) -> Result<ComRc<R>, NTSTATUS>
    where
        R: ComInterface + ComInterfaceInfo<Vtable = I>,
    {
        Self::try_new_rc_in(inner, alloc).ok_or(STATUS_INSUFFICIENT_RESOURCES)
    }

    /// Creates a COM object and returns a smart pointer that owns the initial reference.
    #[inline]
    pub fn try_new_rc_in<R>(inner: T, alloc: A) -> Option<ComRc<R>>
    where
        R: ComInterface + ComInterfaceInfo<Vtable = I>,
    {
        let ptr = Self::try_new_in(inner, alloc)?;
        // SAFETY: `ptr` is a freshly created COM pointer with refcount 1.
        Some(unsafe { ComRc::from_raw_unchecked(ptr as *mut R) })
    }

    #[inline]
    fn non_delegating_ptr(ptr: *mut Self) -> *mut c_void {
        unsafe { &mut (*ptr).non_delegating_unknown as *mut _ as *mut c_void }
    }

    #[inline]
    pub fn new_in(inner: T, alloc: A) -> Result<*mut c_void, NTSTATUS> {
        Self::try_new_in(inner, alloc).ok_or(STATUS_INSUFFICIENT_RESOURCES)
    }

    #[inline]
    pub fn try_new_in(inner: T, alloc: A) -> Option<*mut c_void> {
        let ptr = unsafe { alloc.alloc(Self::LAYOUT) } as *mut Self;
        if ptr.is_null() {
            return None;
        }
        unsafe {
            ptr.write(Self {
                vtable: T::VTABLE,
                non_delegating_unknown: NonDelegatingIUnknown {
                    vtable: &Self::NON_DELEGATING_VTABLE,
                    parent: core::ptr::null_mut(),
                },
                ref_count: AtomicU32::new(1),
                outer_unknown: None,
                inner,
                alloc: ManuallyDrop::new(alloc),
            });
            Self::init_non_delegating_ptr(ptr);
            Some(ptr as *mut c_void)
        }
    }

    #[inline]
    pub fn try_new_in_with_layout(inner: T, alloc: A, layout: Layout) -> Option<*mut c_void> {
        if layout != Self::LAYOUT {
            return None;
        }
        Self::try_new_in(inner, alloc)
    }

    /// Creates an aggregated COM object and returns the **non-delegating IUnknown** pointer.
    ///
    /// The outer object should hold this pointer to manage the inner object's lifetime.
    /// Interfaces returned via `QueryInterface` will delegate IUnknown calls to the outer
    /// unknown, while this non-delegating pointer updates the inner refcount directly.
    ///
    /// # Safety
    /// `outer_unknown` must point to a valid outer IUnknown vtable.
    #[inline]
    pub fn new_aggregated_in(
        inner: T,
        outer_unknown: *mut IUnknownVtbl,
        alloc: A,
    ) -> Result<*mut c_void, NTSTATUS> {
        Self::try_new_aggregated_in(inner, outer_unknown, alloc)
            .ok_or(STATUS_INSUFFICIENT_RESOURCES)
    }

    #[inline]
    pub fn try_new_aggregated_in(
        inner: T,
        outer_unknown: *mut IUnknownVtbl,
        alloc: A,
    ) -> Option<*mut c_void> {
        let ptr = unsafe { alloc.alloc(Self::LAYOUT) } as *mut Self;
        if ptr.is_null() {
            return None;
        }
        unsafe {
            ptr.write(Self {
                vtable: T::VTABLE,
                non_delegating_unknown: NonDelegatingIUnknown {
                    vtable: &Self::NON_DELEGATING_VTABLE,
                    parent: core::ptr::null_mut(),
                },
                ref_count: AtomicU32::new(1),
                outer_unknown: Some(outer_unknown),
                inner,
                alloc: ManuallyDrop::new(alloc),
            });
            Self::init_non_delegating_ptr(ptr);
            Some(Self::non_delegating_ptr(ptr))
        }
    }

    #[inline]
    pub fn is_aggregated(&self) -> bool {
        self.outer_unknown.is_some()
    }

    #[inline]
    pub fn inner_ref(&self) -> &T {
        &self.inner
    }

    #[inline]
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    #[inline(always)]
    /// # Safety
    /// `ptr` must be a valid pointer to a `ComObject<T, I>` allocated by this crate.
    /// The pointer must be properly aligned and remain valid for the returned lifetime.
    pub unsafe fn from_ptr<'a>(ptr: *mut c_void) -> &'a Self {
        unsafe { &*(ptr as *const Self) }
    }

    #[inline(always)]
    /// # Safety
    /// `ptr` must be a valid pointer to a non-delegating IUnknown created by this crate.
    pub unsafe fn from_non_delegating<'a>(ptr: *mut c_void) -> &'a Self {
        let unknown = unsafe { &*(ptr as *const NonDelegatingIUnknown<T, I, A>) };
        unsafe { &*unknown.parent }
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid COM pointer created by `ComObject` for `T`.
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
    /// # Safety
    /// `this` must be a valid COM pointer created by `ComObject` for `T`.
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
            let ptr = this as *mut Self;
            let alloc = unsafe { core::ptr::read(&(*ptr).alloc) };
            let alloc = ManuallyDrop::into_inner(alloc);
            unsafe {
                core::ptr::drop_in_place(ptr);
                alloc.dealloc(ptr as *mut u8, Self::LAYOUT);
            }
            drop(alloc);
        }

        count
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid COM pointer created by `ComObject` for `T`.
    /// `riid` and `ppv` must be valid, non-null pointers.
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

        if let Some(ptr) = wrapper.inner.query_interface(this, riid) {
            let vtbl = unsafe { *(ptr as *mut *mut IUnknownVtbl) };
            unsafe { ((*vtbl).AddRef)(ptr) };
            unsafe { *ppv = ptr };
            return STATUS_SUCCESS;
        }

        unsafe { *ppv = core::ptr::null_mut() };
        STATUS_NOINTERFACE
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid non-delegating IUnknown pointer created by `ComObject` for `T`.
    pub unsafe extern "system" fn shim_non_delegating_add_ref(this: *mut c_void) -> u32 {
        let wrapper = unsafe { Self::from_non_delegating(this) };
        wrapper.ref_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid non-delegating IUnknown pointer created by `ComObject` for `T`.
    pub unsafe extern "system" fn shim_non_delegating_release(this: *mut c_void) -> u32 {
        let wrapper = unsafe { Self::from_non_delegating(this) };
        let count = wrapper.ref_count.fetch_sub(1, Ordering::Release) - 1;

        if count == 0 {
            core::sync::atomic::fence(Ordering::Acquire);
            let ptr = wrapper as *const Self as *mut Self;
            let alloc = unsafe { core::ptr::read(&(*ptr).alloc) };
            let alloc = ManuallyDrop::into_inner(alloc);
            unsafe {
                core::ptr::drop_in_place(ptr);
                alloc.dealloc(ptr as *mut u8, Self::LAYOUT);
            }
            drop(alloc);
        }

        count
    }

    #[allow(non_snake_case)]
    /// # Safety
    /// `this` must be a valid non-delegating IUnknown pointer created by `ComObject` for `T`.
    /// `riid` and `ppv` must be valid, non-null pointers.
    pub unsafe extern "system" fn shim_non_delegating_query_interface(
        this: *mut c_void,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> NTSTATUS {
        let wrapper = unsafe { Self::from_non_delegating(this) };

        if ppv.is_null() || riid.is_null() {
            return STATUS_NOINTERFACE;
        }

        let riid = unsafe { &*riid };

        if *riid == IID_IUNKNOWN {
            unsafe { Self::shim_non_delegating_add_ref(this) };
            unsafe { *ppv = this };
            return STATUS_SUCCESS;
        }

        let delegating_ptr = wrapper as *const Self as *mut c_void;

        if let Some(ptr) = wrapper.inner.query_interface(delegating_ptr, riid) {
            let vtbl = unsafe { *(ptr as *mut *mut IUnknownVtbl) };
            unsafe { ((*vtbl).AddRef)(ptr) };
            unsafe { *ppv = ptr };
            return STATUS_SUCCESS;
        }

        unsafe { *ppv = core::ptr::null_mut() };
        STATUS_NOINTERFACE
    }
}

impl<T, I> ComObject<T, I, GlobalAllocator>
where
    T: ComImpl<I>,
    I: InterfaceVtable,
{
    #[inline]
    pub fn new(inner: T) -> Result<*mut c_void, NTSTATUS> {
        Self::new_in(inner, GlobalAllocator)
    }

    #[inline]
    pub fn try_new(inner: T) -> Option<*mut c_void> {
        Self::try_new_in(inner, GlobalAllocator)
    }

    /// Creates a COM object and returns a smart pointer that owns the initial reference.
    #[inline]
    pub fn new_rc<R>(inner: T) -> Result<ComRc<R>, NTSTATUS>
    where
        R: ComInterface + ComInterfaceInfo<Vtable = I>,
    {
        Self::new_rc_in(inner, GlobalAllocator)
    }

    /// Creates a COM object and returns a smart pointer that owns the initial reference.
    #[inline]
    pub fn try_new_rc<R>(inner: T) -> Option<ComRc<R>>
    where
        R: ComInterface + ComInterfaceInfo<Vtable = I>,
    {
        Self::try_new_rc_in(inner, GlobalAllocator)
    }

    /// # Safety
    /// `outer_unknown` must point to a valid outer IUnknown vtable.
    #[inline]
    pub fn new_aggregated(inner: T, outer_unknown: *mut IUnknownVtbl) -> Result<*mut c_void, NTSTATUS> {
        Self::new_aggregated_in(inner, outer_unknown, GlobalAllocator)
    }

    /// # Safety
    /// `outer_unknown` must point to a valid outer IUnknown vtable.
    #[inline]
    pub fn try_new_aggregated(
        inner: T,
        outer_unknown: *mut IUnknownVtbl,
    ) -> Option<*mut c_void> {
        Self::try_new_aggregated_in(inner, outer_unknown, GlobalAllocator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use core::sync::atomic::{AtomicU32, Ordering};

    static DROP_COUNT: AtomicU32 = AtomicU32::new(0);
    static OUTER_ADDREF_COUNT: AtomicU32 = AtomicU32::new(0);
    static OUTER_RELEASE_COUNT: AtomicU32 = AtomicU32::new(0);
    static OUTER_QUERY_COUNT: AtomicU32 = AtomicU32::new(0);

    unsafe extern "system" fn outer_add_ref(_this: *mut core::ffi::c_void) -> u32 {
        OUTER_ADDREF_COUNT.fetch_add(1, Ordering::Relaxed) + 1
    }

    unsafe extern "system" fn outer_release(_this: *mut core::ffi::c_void) -> u32 {
        OUTER_RELEASE_COUNT.fetch_add(1, Ordering::Relaxed) + 1
    }

    unsafe extern "system" fn outer_query_interface(
        _this: *mut core::ffi::c_void,
        _riid: *const GUID,
        ppv: *mut *mut core::ffi::c_void,
    ) -> NTSTATUS {
        OUTER_QUERY_COUNT.fetch_add(1, Ordering::Relaxed);
        if !ppv.is_null() {
            unsafe { *ppv = core::ptr::null_mut() };
        }
        STATUS_NOINTERFACE
    }

    static OUTER_VTBL: IUnknownVtbl = IUnknownVtbl {
        QueryInterface: outer_query_interface,
        AddRef: outer_add_ref,
        Release: outer_release,
    };

    struct Dummy;

    impl Drop for Dummy {
        fn drop(&mut self) {
            DROP_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn add_ref_release_drops() {
        DROP_COUNT.store(0, Ordering::Relaxed);

        let ptr = ComObject::<Dummy, IUnknownVtbl>::new(Dummy).unwrap();

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

        let ptr = ComObject::<Dummy, IUnknownVtbl>::new(Dummy).unwrap();
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

    #[test]
    fn aggregated_non_delegating_and_delegating_paths() {
        DROP_COUNT.store(0, Ordering::Relaxed);
        OUTER_ADDREF_COUNT.store(0, Ordering::Relaxed);
        OUTER_RELEASE_COUNT.store(0, Ordering::Relaxed);
        OUTER_QUERY_COUNT.store(0, Ordering::Relaxed);

        let ptr = ComObject::<Dummy, IUnknownVtbl>::new_aggregated(
            Dummy,
            &OUTER_VTBL as *const _ as *mut _,
        )
        .unwrap();

        unsafe {
            let vtbl = *(ptr as *mut *mut IUnknownVtbl);
            assert_eq!(((*vtbl).AddRef)(ptr), 2);
            assert_eq!(((*vtbl).Release)(ptr), 1);
        }

        assert_eq!(OUTER_ADDREF_COUNT.load(Ordering::Relaxed), 0);
        assert_eq!(OUTER_RELEASE_COUNT.load(Ordering::Relaxed), 0);

        let delegating_ptr = unsafe {
            ComObject::<Dummy, IUnknownVtbl>::from_non_delegating(ptr) as *const _
                as *mut core::ffi::c_void
        };

        unsafe {
            assert_eq!(
                ComObject::<Dummy, IUnknownVtbl>::shim_add_ref(delegating_ptr),
                1
            );
            assert_eq!(
                ComObject::<Dummy, IUnknownVtbl>::shim_release(delegating_ptr),
                1
            );
        }

        assert_eq!(OUTER_ADDREF_COUNT.load(Ordering::Relaxed), 1);
        assert_eq!(OUTER_RELEASE_COUNT.load(Ordering::Relaxed), 1);

        unsafe {
            let vtbl = *(ptr as *mut *mut IUnknownVtbl);
            assert_eq!(((*vtbl).Release)(ptr), 0);
        }

        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 1);
    }
}
