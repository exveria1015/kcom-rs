// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::alloc::Layout;
use core::mem::ManuallyDrop;
use core::pin::Pin;
use core::ptr;
use core::ptr::NonNull;
use core::marker::PhantomData;
#[cfg(feature = "driver")]
use core::ffi::c_void;
#[cfg(feature = "driver")]
use core::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "driver")]
use wdk_sys::ntddk::{KeGetCurrentIrql, MmGetSystemRoutineAddress};
#[cfg(feature = "driver")]
use wdk_sys::{UNICODE_STRING, PASSIVE_LEVEL};

use crate::iunknown::{NTSTATUS, Status, STATUS_INSUFFICIENT_RESOURCES};

pub trait Allocator {
    /// # Safety
    /// `layout` must be valid.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8;

    /// # Safety
    /// `layout` must be valid.
    /// The returned memory is zero-initialized.
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.alloc(layout) };
        if !ptr.is_null() {
            unsafe { ptr::write_bytes(ptr, 0, layout.size()) };
        }
        ptr
    }

    /// # Safety
    /// `ptr` must have been allocated by this allocator with the same `layout`.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout);
}

/// Fallible allocation helper that returns `STATUS_INSUFFICIENT_RESOURCES` on OOM.
#[inline]
pub fn try_alloc_layout<A: Allocator>(alloc: &A, layout: Layout) -> Result<NonNull<u8>, NTSTATUS> {
    let ptr = unsafe { alloc.alloc(layout) };
    NonNull::new(ptr).ok_or(STATUS_INSUFFICIENT_RESOURCES)
}

/// Fallible allocation helper that writes `value` into allocator-owned memory.
#[inline]
pub fn try_alloc_value_in<T, A: Allocator>(alloc: &A, value: T) -> Result<NonNull<T>, NTSTATUS> {
    let layout = Layout::new::<T>();
    let ptr = unsafe { alloc.alloc(layout) } as *mut T;
    let ptr = NonNull::new(ptr).ok_or(STATUS_INSUFFICIENT_RESOURCES)?;
    unsafe {
        ptr.as_ptr().write(value);
    }
    Ok(ptr)
}

pub enum KBoxError<E> {
    Alloc(NTSTATUS),
    Init(E),
}

impl<E> KBoxError<E>
where
    E: Into<NTSTATUS>,
{
    #[inline]
    pub fn into_status(self) -> NTSTATUS {
        match self {
            Self::Alloc(status) => status,
            Self::Init(err) => err.into(),
        }
    }
}

impl<E> From<KBoxError<E>> for NTSTATUS
where
    E: Into<NTSTATUS>,
{
    #[inline]
    fn from(err: KBoxError<E>) -> Self {
        err.into_status()
    }
}

impl<E> From<KBoxError<E>> for Status
where
    E: Into<NTSTATUS>,
{
    #[inline]
    fn from(err: KBoxError<E>) -> Self {
        Status::from(err.into_status())
    }
}

pub trait PinInit<T, E> {
    /// # Safety
    /// `ptr` must be valid for writes and aligned for `T`.
    unsafe fn init(&mut self, ptr: *mut T) -> Result<(), E>;
}

pub struct PinInitOnce<F> {
    init: Option<F>,
}

impl<F> PinInitOnce<F> {
    #[inline]
    pub fn new(init: F) -> Self {
        Self { init: Some(init) }
    }
}

impl<T, E, F> PinInit<T, E> for PinInitOnce<F>
where
    E: From<NTSTATUS>,
    F: FnOnce(*mut T) -> Result<(), E>,
{
    #[inline]
    unsafe fn init(&mut self, ptr: *mut T) -> Result<(), E> {
        let Some(init) = self.init.take() else {
            return Err(E::from(crate::iunknown::STATUS_UNSUCCESSFUL));
        };
        init(ptr)
    }
}

impl<T, E> PinInit<T, E> for crate::alloc::boxed::Box<dyn PinInit<T, E> + '_> {
    #[inline]
    unsafe fn init(&mut self, ptr: *mut T) -> Result<(), E> {
        (**self).init(ptr)
    }
}

pub struct KBox<T: ?Sized, A: Allocator = GlobalAllocator> {
    ptr: NonNull<T>,
    alloc: ManuallyDrop<A>,
    layout: Layout,
}

impl<T, A: Allocator> KBox<T, A> {
    #[inline]
    pub fn try_pin_init<E>(alloc: A, mut init: impl PinInit<T, E>) -> Result<Pin<Self>, KBoxError<E>> {
        let layout = Layout::new::<T>();
        let ptr = unsafe { alloc.alloc(layout) } as *mut T;
        let ptr = NonNull::new(ptr).ok_or(KBoxError::Alloc(STATUS_INSUFFICIENT_RESOURCES))?;
        unsafe {
            if let Err(err) = init.init(ptr.as_ptr()) {
                alloc.dealloc(ptr.as_ptr() as *mut u8, layout);
                return Err(KBoxError::Init(err));
            }
        }
        // SAFETY: value is initialized in-place and owned by this KBox.
        Ok(unsafe { Pin::new_unchecked(Self::from_raw_parts(ptr, alloc, layout)) })
    }

    /// # Safety
    /// Caller must ensure `ptr` was allocated with `alloc` using `layout` and is
    /// a valid instance of `T` that can be dropped.
    #[inline]
    pub(crate) unsafe fn from_raw_parts(ptr: NonNull<T>, alloc: A, layout: Layout) -> Self {
        Self {
            ptr,
            alloc: ManuallyDrop::new(alloc),
            layout,
        }
    }

    #[inline]
    pub fn into_raw_parts(self) -> (NonNull<T>, A, Layout) {
        let mut this = ManuallyDrop::new(self);
        let ptr = this.ptr;
        let alloc = unsafe { ManuallyDrop::take(&mut this.alloc) };
        let layout = this.layout;
        (ptr, alloc, layout)
    }

    #[inline]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }
}

impl<T: ?Sized, A: Allocator> core::ops::Deref for KBox<T, A> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.as_ref() }
    }
}

impl<T: ?Sized, A: Allocator> core::ops::DerefMut for KBox<T, A> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.ptr.as_mut() }
    }
}

impl<T: ?Sized, A: Allocator> Drop for KBox<T, A> {
    fn drop(&mut self) {
        unsafe {
            ptr::drop_in_place(self.ptr.as_ptr());
            let alloc = ManuallyDrop::take(&mut self.alloc);
            alloc.dealloc(self.ptr.as_ptr() as *mut u8, self.layout);
        }
    }
}

pub trait InitBoxTrait<T, A: Allocator, E> {
    fn try_pin(self) -> Result<Pin<KBox<T, A>>, KBoxError<E>>;
}

pub struct InitBox<T, A: Allocator, E, I> {
    alloc: A,
    init: I,
    _marker: PhantomData<fn() -> (T, E)>,
}

impl<T, A: Allocator, E, I> InitBox<T, A, E, I>
where
    I: PinInit<T, E>,
{
    #[inline]
    pub fn new(alloc: A, init: I) -> Self {
        Self {
            alloc,
            init,
            _marker: PhantomData,
        }
    }
}

impl<T, A: Allocator, E, I> InitBoxTrait<T, A, E> for InitBox<T, A, E, I>
where
    I: PinInit<T, E>,
{
    #[inline]
    fn try_pin(self) -> Result<Pin<KBox<T, A>>, KBoxError<E>> {
        KBox::try_pin_init(self.alloc, self.init)
    }
}

#[cfg(feature = "driver")]
pub type KernelInitBox<T, E, I> = InitBox<T, WdkAllocator, E, I>;

#[cfg(feature = "driver")]
#[inline]
pub fn init_box_with_tag<'a, T, E>(
    pool: PoolType,
    tag: u32,
    init: impl PinInit<T, E> + 'a,
) -> KernelInitBox<T, E, impl PinInit<T, E> + 'a> {
    InitBox::new(WdkAllocator::new(pool, tag), init)
}

pub struct GlobalAllocator;

impl Allocator for GlobalAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        alloc::alloc::alloc(layout)
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        alloc::alloc::dealloc(ptr, layout)
    }
}

#[cfg(feature = "driver")]
#[derive(Copy, Clone)]
pub enum PoolType {
    NonPagedNx,
    Paged,
}

#[cfg(feature = "driver")]
#[derive(Copy, Clone)]
pub struct WdkAllocator {
    pub pool: PoolType,
    pub tag: u32,
}

#[cfg(feature = "driver")]
impl WdkAllocator {
    #[inline]
    pub const fn new(pool: PoolType, tag: u32) -> Self {
        Self { pool, tag }
    }

    /// Allocate memory without zeroing. Caller must fully initialize the buffer.
    #[inline]
    pub unsafe fn alloc_uninitialized(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return core::ptr::NonNull::<u8>::dangling().as_ptr();
        }

        let ptr = unsafe { ex_allocate_pool_uninitialized(self.pool, layout.size(), self.tag) };
        ptr as *mut u8
    }
}

#[cfg(feature = "driver")]
impl Allocator for WdkAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return core::ptr::NonNull::<u8>::dangling().as_ptr();
        }

        let ptr = unsafe { ex_allocate_pool_uninitialized(self.pool, layout.size(), self.tag) };
        ptr as *mut u8
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return core::ptr::NonNull::<u8>::dangling().as_ptr();
        }

        let ptr = unsafe { ex_allocate_pool(self.pool, layout.size(), self.tag) };
        ptr as *mut u8
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if layout.size() == 0 {
            return;
        }
        unsafe { ExFreePoolWithTag(ptr as _, self.tag) }
    }
}

#[cfg(feature = "driver")]
const POOL_FLAG_PAGED: u64 = 0x0000_0001;
#[cfg(feature = "driver")]
const POOL_FLAG_UNINITIALIZED: u64 = 0x0000_0002;
#[cfg(feature = "driver")]
const POOL_FLAG_NON_PAGED: u64 = 0x0000_0040;

#[cfg(feature = "driver")]
const POOL_TYPE_PAGED: u32 = 1;
#[cfg(feature = "driver")]
const POOL_TYPE_NON_PAGED_NX: u32 = 512;

#[cfg(feature = "driver")]
type ExAllocatePool2Fn = unsafe extern "C" fn(u64, usize, u32) -> *mut c_void;

#[cfg(feature = "driver")]
const EX_ALLOCATE_POOL2_NAME: [u16; 16] = [
    b'E' as u16,
    b'x' as u16,
    b'A' as u16,
    b'l' as u16,
    b'l' as u16,
    b'o' as u16,
    b'c' as u16,
    b'a' as u16,
    b't' as u16,
    b'e' as u16,
    b'P' as u16,
    b'o' as u16,
    b'o' as u16,
    b'l' as u16,
    b'2' as u16,
    0,
];

#[cfg(feature = "driver")]
const EX_ALLOCATE_POOL2_STATE_UNINIT: usize = 0;
#[cfg(feature = "driver")]
const EX_ALLOCATE_POOL2_STATE_INITING: usize = 1;
#[cfg(feature = "driver")]
const EX_ALLOCATE_POOL2_STATE_READY: usize = 2;
#[cfg(feature = "driver")]
static EX_ALLOCATE_POOL2_PTR: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "driver")]
static EX_ALLOCATE_POOL2_STATE: AtomicUsize = AtomicUsize::new(EX_ALLOCATE_POOL2_STATE_UNINIT);

/// Resolve ExAllocatePool2 at PASSIVE_LEVEL (e.g. DriverEntry) and cache it.
///
/// Calling this in DriverEntry ensures ExAllocatePool2 is used even when later
/// allocations happen at elevated IRQL. Allocations will lazily attempt to
/// resolve the routine if it has not been initialized yet, but that best-effort
/// path may occur at unsuitable IRQLs, so prefer explicit initialization.
#[cfg(feature = "driver")]
#[inline]
pub unsafe fn init_ex_allocate_pool2() {
    unsafe { try_init_ex_allocate_pool2() };
}

#[cfg(feature = "driver")]
unsafe fn ex_allocate_pool(pool: PoolType, size: usize, tag: u32) -> *mut c_void {
    let flags = match pool {
        PoolType::NonPagedNx => POOL_FLAG_NON_PAGED,
        PoolType::Paged => POOL_FLAG_PAGED,
    };
    if let Some(func) = unsafe { get_ex_allocate_pool2() } {
        return unsafe { func(flags, size, tag) };
    }
    let pool_type = match pool {
        PoolType::NonPagedNx => POOL_TYPE_NON_PAGED_NX,
        PoolType::Paged => POOL_TYPE_PAGED,
    };
    let ptr = unsafe { ExAllocatePoolWithTag(pool_type, size, tag) };
    if !ptr.is_null() {
        unsafe { ptr::write_bytes(ptr, 0, size) };
    }
    ptr
}

#[cfg(feature = "driver")]
unsafe fn ex_allocate_pool_uninitialized(pool: PoolType, size: usize, tag: u32) -> *mut c_void {
    let flags = match pool {
        PoolType::NonPagedNx => POOL_FLAG_NON_PAGED,
        PoolType::Paged => POOL_FLAG_PAGED,
    } | POOL_FLAG_UNINITIALIZED;
    if let Some(func) = unsafe { get_ex_allocate_pool2() } {
        return unsafe { func(flags, size, tag) };
    }
    let pool_type = match pool {
        PoolType::NonPagedNx => POOL_TYPE_NON_PAGED_NX,
        PoolType::Paged => POOL_TYPE_PAGED,
    };
    unsafe { ExAllocatePoolWithTag(pool_type, size, tag) }
}

#[cfg(feature = "driver")]
unsafe fn try_init_ex_allocate_pool2() {
    let irql = unsafe { KeGetCurrentIrql() };
    if irql > PASSIVE_LEVEL as u8 {
        return;
    }
    if EX_ALLOCATE_POOL2_STATE
        .compare_exchange(
            EX_ALLOCATE_POOL2_STATE_UNINIT,
            EX_ALLOCATE_POOL2_STATE_INITING,
            Ordering::Acquire,
            Ordering::Acquire,
        )
        .is_err()
    {
        return;
    }

    let mut name = UNICODE_STRING {
        Length: 30,
        MaximumLength: 32,
        Buffer: EX_ALLOCATE_POOL2_NAME.as_ptr() as *mut u16,
    };
    let ptr = unsafe { MmGetSystemRoutineAddress(&mut name) };
    EX_ALLOCATE_POOL2_PTR.store(ptr as usize, Ordering::Release);
    EX_ALLOCATE_POOL2_STATE.store(EX_ALLOCATE_POOL2_STATE_READY, Ordering::Release);
}

#[cfg(feature = "driver")]
unsafe fn get_ex_allocate_pool2() -> Option<ExAllocatePool2Fn> {
    if EX_ALLOCATE_POOL2_STATE.load(Ordering::Acquire) != EX_ALLOCATE_POOL2_STATE_READY {
        unsafe { try_init_ex_allocate_pool2() };
    }
    if EX_ALLOCATE_POOL2_STATE.load(Ordering::Acquire) != EX_ALLOCATE_POOL2_STATE_READY {
        return None;
    }
    let ptr = EX_ALLOCATE_POOL2_PTR.load(Ordering::Acquire);
    if ptr == 0 {
        None
    } else {
        Some(unsafe { core::mem::transmute(ptr) })
    }
}

#[cfg(feature = "driver")]
unsafe extern "C" {
    fn ExAllocatePoolWithTag(pool_type: u32, number_of_bytes: usize, tag: u32) -> *mut c_void;
    fn ExFreePoolWithTag(p: *mut c_void, tag: u32);
}
