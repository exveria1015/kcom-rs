// async_com/fused.rs
//
// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::future::Future;
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use super::{AsyncOperationRaw, AsyncOperationVtbl, AsyncStatus, AsyncValueType, ReleaseGuard};
use crate::allocator::{Allocator, InitBoxTrait, PinInit, PinInitOnce};
use crate::iunknown::{
    GUID, IUnknownVtbl, NTSTATUS, STATUS_CANCELLED, STATUS_INSUFFICIENT_RESOURCES,
    STATUS_NOINTERFACE, STATUS_PENDING, STATUS_SUCCESS, STATUS_UNSUCCESSFUL, IID_IUNKNOWN,
};
use crate::ntddk::{KeGetCurrentIrql, KeInitializeDpc, KeInsertQueueDpc, KDPC, PKDPC};
use crate::refcount;
use crate::wrapper::PanicGuard;

use wdk_sys::{NPAGED_LOOKASIDE_LIST, PNPAGED_LOOKASIDE_LIST, POOL_TYPE, PVOID, SIZE_T, ULONG, PASSIVE_LEVEL};
use wdk_sys::ntddk::{
    ExAllocateFromNPagedLookasideList, ExFreeToNPagedLookasideList, ExInitializeNPagedLookasideList,
};

const STATUS_MASK: u32 = 0x0000_FFFF;
const FLAG_POLLING: u32 = 0x8000_0000;
const FLAG_DPC_QUEUED: u32 = 0x4000_0000;
const FLAG_FUTURE_DROPPED: u32 = 0x2000_0000;

const SLAB_ALIGN: usize = 64;
const SLAB_SIZES: [usize; 5] = [128, 256, 512, 1024, 2048];
const SLAB_COUNT: usize = SLAB_SIZES.len();
const SLAB_TAG: u32 = u32::from_ne_bytes(*b"KCFU");
const HEAP_TAG: u32 = u32::from_ne_bytes(*b"KCFH");
const SLAB_HEADER_SIZE: usize = core::mem::size_of::<usize>();

const SLABS_STATE_UNINIT: u32 = 0;
const SLABS_STATE_INITING: u32 = 1;
const SLABS_STATE_READY: u32 = 2;
static SLABS_STATE: AtomicU32 = AtomicU32::new(SLABS_STATE_UNINIT);

#[repr(C, align(64))]
struct TaskHeader<T: AsyncValueType> {
    vtable: *mut AsyncOperationVtbl<T>,
    ref_count: AtomicU32,
    status: AtomicU32,
    result: UnsafeCell<MaybeUninit<T>>,
}

#[repr(C)]
struct TaskBody<F> {
    dpc: KDPC,
    future: TaskFuture<F>,
}

#[repr(C)]
struct FusedTask<T, F>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    header: TaskHeader<T>,
    body: TaskBody<F>,
}

impl<T, F> FusedTask<T, F>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    const VTABLE: AsyncOperationVtbl<T> = AsyncOperationVtbl {
        parent: IUnknownVtbl {
            QueryInterface: Self::shim_query_interface,
            AddRef: Self::shim_add_ref,
            Release: Self::shim_release,
        },
        get_status: Self::shim_get_status,
        get_result: Self::shim_get_result,
    };

    const BIN_INDEX: Option<usize> = select_bin(
        core::mem::size_of::<Self>(),
        core::mem::align_of::<Self>(),
    );

    const _LAYOUT_CHECK: () = {
        assert!(core::mem::offset_of!(FusedTask<T, F>, header) == 0);
        assert!(core::mem::offset_of!(FusedTask<T, F>, body) % SLAB_ALIGN == 0);
    };

    #[inline]
    unsafe fn add_ref(ptr: NonNull<Self>) -> u32 {
        let header = unsafe { &(*ptr.as_ptr()).header };
        refcount::add(&header.ref_count)
    }

    #[inline]
    unsafe fn release(ptr: NonNull<Self>) -> u32 {
        let header = unsafe { &(*ptr.as_ptr()).header };
        let count = refcount::sub(&header.ref_count);
        if count != 0 {
            return count;
        }

        core::sync::atomic::fence(Ordering::Acquire);
        let status = header.status.load(Ordering::Acquire);
        if (status & FLAG_FUTURE_DROPPED) == 0 {
            unsafe {
                core::ptr::drop_in_place(core::ptr::addr_of_mut!((*ptr.as_ptr()).body.future));
            }
        }
        let resurrected = header.ref_count.load(Ordering::Acquire);
        if resurrected != 0 {
            resurrection_violation();
        }
        unsafe { Self::dealloc(ptr) };
        count
    }

    #[inline]
    unsafe fn dealloc(ptr: NonNull<Self>) {
        match Self::BIN_INDEX {
            Some(idx) => unsafe { slab_free_indexed(idx, ptr.as_ptr() as *mut u8) },
            None => unsafe {
                free_aligned(ptr.as_ptr() as *mut u8, HEAP_TAG);
            },
        }
    }

    #[inline]
    unsafe fn complete(ptr: NonNull<Self>, value: T) {
        unsafe {
            (*(*ptr.as_ptr()).header.result.get()).write(value);
        }
        unsafe {
            (*ptr.as_ptr())
                .header
                .status
                .store(AsyncStatus::Completed.as_raw(), Ordering::Release);
        }
        unsafe {
            core::ptr::drop_in_place(core::ptr::addr_of_mut!((*ptr.as_ptr()).body.future));
            (*ptr.as_ptr())
                .header
                .status
                .fetch_or(FLAG_FUTURE_DROPPED, Ordering::Release);
        }
    }

    #[inline]
    unsafe fn try_set_polling(ptr: NonNull<Self>) -> bool {
        let status = unsafe { &(*ptr.as_ptr()).header.status };
        let mut curr = status.load(Ordering::Acquire);
        loop {
            if (curr & FLAG_POLLING) != 0 {
                return false;
            }
            if (curr & STATUS_MASK) != AsyncStatus::Started.as_raw() {
                return false;
            }
            let next = curr | FLAG_POLLING;
            match status.compare_exchange(curr, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return true,
                Err(next) => curr = next,
            }
        }
    }

    #[inline]
    unsafe fn finish_pending(ptr: NonNull<Self>) {
        let header = unsafe { &(*ptr.as_ptr()).header };
        let prev = header
            .status
            .fetch_and(!(FLAG_POLLING | FLAG_DPC_QUEUED), Ordering::AcqRel);
        if (prev & FLAG_DPC_QUEUED) != 0 {
            unsafe {
                KeInsertQueueDpc(
                    &mut (*ptr.as_ptr()).body.dpc as PKDPC,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                );
            }
        }
    }

    #[inline]
    unsafe fn wake(ptr: NonNull<Self>) {
        let header = unsafe { &(*ptr.as_ptr()).header };
        let status = header.status.load(Ordering::Acquire);
        if (status & STATUS_MASK) != AsyncStatus::Started.as_raw() {
            return;
        }

        let prev = header.status.fetch_or(FLAG_DPC_QUEUED, Ordering::AcqRel);
        if (prev & FLAG_DPC_QUEUED) != 0 {
            return;
        }

        if (prev & FLAG_POLLING) == 0 {
            unsafe {
                KeInsertQueueDpc(
                    &mut (*ptr.as_ptr()).body.dpc as PKDPC,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                );
            }
        }
    }

    const RAW_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        Self::waker_clone,
        Self::waker_wake,
        Self::waker_wake_by_ref,
        Self::waker_drop,
    );

    #[inline]
    unsafe fn poll_inner(ptr: NonNull<Self>) -> Poll<T> {
        let task = unsafe { &mut *ptr.as_ptr() };
        let waker = unsafe { Waker::from_raw(Self::raw_waker(ptr)) };
        let mut cx = Context::from_waker(&waker);
        let future = unsafe { Pin::new_unchecked(&mut task.body.future) };
        future.poll(&mut cx)
    }

    #[inline]
    unsafe fn raw_waker(ptr: NonNull<Self>) -> RawWaker {
        unsafe { Self::add_ref(ptr) };
        RawWaker::new(ptr.as_ptr() as *const (), &Self::RAW_WAKER_VTABLE)
    }

    unsafe fn waker_clone(data: *const ()) -> RawWaker {
        let ptr = unsafe { NonNull::new_unchecked(data as *mut Self) };
        unsafe { Self::add_ref(ptr) };
        RawWaker::new(data, &Self::RAW_WAKER_VTABLE)
    }

    unsafe fn waker_wake(data: *const ()) {
        let ptr = unsafe { NonNull::new_unchecked(data as *mut Self) };
        unsafe {
            Self::wake(ptr);
            Self::release(ptr);
        }
    }

    unsafe fn waker_wake_by_ref(data: *const ()) {
        let ptr = unsafe { NonNull::new_unchecked(data as *mut Self) };
        unsafe { Self::wake(ptr) };
    }

    unsafe fn waker_drop(data: *const ()) {
        let ptr = unsafe { NonNull::new_unchecked(data as *mut Self) };
        unsafe { Self::release(ptr) };
    }

    unsafe extern "C" fn dpc_callback(
        _dpc: PKDPC,
        deferred_context: *mut c_void,
        _system_argument1: *mut c_void,
        _system_argument2: *mut c_void,
    ) {
        let ptr = match NonNull::new(deferred_context as *mut Self) {
            Some(ptr) => ptr,
            None => return,
        };

        let status = unsafe { &(*ptr.as_ptr()).header.status }.load(Ordering::Acquire);
        if (status & STATUS_MASK) != AsyncStatus::Started.as_raw() {
            return;
        }

        if !unsafe { Self::try_set_polling(ptr) } {
            return;
        }

        let poll = unsafe { Self::poll_inner(ptr) };
        match poll {
            Poll::Ready(value) => {
                unsafe { Self::complete(ptr, value) };
                unsafe { Self::release(ptr) };
            }
            Poll::Pending => unsafe { Self::finish_pending(ptr) },
        }
    }

    fn spawn_raw(future: F) -> Result<*mut AsyncOperationRaw<T>, NTSTATUS> {
        let mut init = PinInitOnce::new(|ptr: *mut TaskFuture<F>| {
            // SAFETY: caller guarantees `ptr` is valid for writes.
            unsafe {
                ptr.write(TaskFuture {
                    guard: None,
                    future,
                });
            }
            Ok(())
        });
        spawn_with_init::<T, F, _>(&mut init)
    }

    fn spawn_raw_with_init<A>(
        init: impl InitBoxTrait<F, A, NTSTATUS>,
        guard: Option<ReleaseGuard>,
    ) -> Result<*mut AsyncOperationRaw<T>, NTSTATUS>
    where
        A: Allocator + Send + Sync,
    {
        let (_alloc, init) = init.into_components();
        let mut init = TaskFutureInit::new(init, guard);
        spawn_with_init::<T, F, _>(&mut init)
    }

    #[allow(non_snake_case)]
    pub unsafe extern "system" fn shim_query_interface(
        this: *mut c_void,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> NTSTATUS {
        let guard = PanicGuard::new();
        if this.is_null() || riid.is_null() || ppv.is_null() {
            core::mem::forget(guard);
            return STATUS_NOINTERFACE;
        }

        let riid = unsafe { &*riid };
        if *riid == IID_IUNKNOWN {
            unsafe { Self::shim_add_ref(this) };
            unsafe { *ppv = this };
            core::mem::forget(guard);
            return STATUS_SUCCESS;
        }

        unsafe { *ppv = core::ptr::null_mut() };
        core::mem::forget(guard);
        STATUS_NOINTERFACE
    }

    #[allow(non_snake_case)]
    pub unsafe extern "system" fn shim_add_ref(this: *mut c_void) -> u32 {
        let guard = PanicGuard::new();
        if this.is_null() {
            core::mem::forget(guard);
            return 0;
        }
        let ptr = unsafe { NonNull::new_unchecked(this as *mut Self) };
        let result = unsafe { Self::add_ref(ptr) };
        core::mem::forget(guard);
        result
    }

    #[allow(non_snake_case)]
    pub unsafe extern "system" fn shim_release(this: *mut c_void) -> u32 {
        let guard = PanicGuard::new();
        if this.is_null() {
            core::mem::forget(guard);
            return 0;
        }
        let ptr = unsafe { NonNull::new_unchecked(this as *mut Self) };
        let result = unsafe { Self::release(ptr) };
        core::mem::forget(guard);
        result
    }

    #[allow(non_snake_case)]
    pub unsafe extern "system" fn shim_get_status(
        this: *mut c_void,
        out_status: *mut AsyncStatus,
    ) -> NTSTATUS {
        if this.is_null() || out_status.is_null() {
            return STATUS_UNSUCCESSFUL;
        }
        let guard = PanicGuard::new();
        let ptr = unsafe { &*(this as *const Self) };
        let raw = ptr.header.status.load(Ordering::Acquire) & STATUS_MASK;
        unsafe {
            *out_status = AsyncStatus::from_raw(raw);
        }
        let result = STATUS_SUCCESS;
        core::mem::forget(guard);
        result
    }

    #[allow(non_snake_case)]
    pub unsafe extern "system" fn shim_get_result(
        this: *mut c_void,
        out_result: *mut T,
    ) -> NTSTATUS {
        if this.is_null() || out_result.is_null() {
            return STATUS_UNSUCCESSFUL;
        }
        let guard = PanicGuard::new();
        let ptr = unsafe { &*(this as *const Self) };
        let raw = ptr.header.status.load(Ordering::Acquire) & STATUS_MASK;
        let status = AsyncStatus::from_raw(raw);
        let result = match status {
            AsyncStatus::Completed => {
                let value = unsafe { (*ptr.header.result.get()).assume_init() };
                unsafe { out_result.write(value) };
                STATUS_SUCCESS
            }
            AsyncStatus::Started => STATUS_PENDING,
            AsyncStatus::Canceled => STATUS_CANCELLED,
            AsyncStatus::Error => STATUS_UNSUCCESSFUL,
        };
        core::mem::forget(guard);
        result
    }
}

pub(super) fn spawn_raw<T, F>(future: F) -> Result<*mut AsyncOperationRaw<T>, NTSTATUS>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    FusedTask::<T, F>::spawn_raw(future)
}

pub(super) fn spawn_raw_with_init<T, F, A>(
    init: impl InitBoxTrait<F, A, NTSTATUS>,
    guard: Option<ReleaseGuard>,
) -> Result<*mut AsyncOperationRaw<T>, NTSTATUS>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
    A: Allocator + Send + Sync,
{
    FusedTask::<T, F>::spawn_raw_with_init(init, guard)
}

struct TaskFuture<F> {
    guard: Option<ReleaseGuard>,
    future: F,
}

impl<F: Future> Future for TaskFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let future = unsafe { Pin::new_unchecked(&mut this.future) };
        match future.poll(cx) {
            Poll::Ready(value) => {
                if let Some(guard) = this.guard.take() {
                    drop(guard);
                }
                Poll::Ready(value)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<F> Drop for TaskFuture<F> {
    fn drop(&mut self) {
        let _ = self.guard.take();
    }
}

struct TaskFutureInit<I> {
    init: I,
    guard: Option<ReleaseGuard>,
}

impl<I> TaskFutureInit<I> {
    #[inline]
    fn new(init: I, guard: Option<ReleaseGuard>) -> Self {
        Self { init, guard }
    }
}

impl<F, E, I> PinInit<TaskFuture<F>, E> for TaskFutureInit<I>
where
    I: PinInit<F, E>,
{
    unsafe fn init(&mut self, ptr: *mut TaskFuture<F>) -> Result<(), E> {
        let guard = self.guard.take();
        unsafe {
            core::ptr::addr_of_mut!((*ptr).guard).write(guard);
        }
        let future_ptr = unsafe { core::ptr::addr_of_mut!((*ptr).future) };
        let result = unsafe { self.init.init(future_ptr) };
        if result.is_err() {
            unsafe {
                core::ptr::drop_in_place(core::ptr::addr_of_mut!((*ptr).guard));
            }
        }
        result
    }
}

#[inline]
fn spawn_with_init<T, F, I>(init: &mut I) -> Result<*mut AsyncOperationRaw<T>, NTSTATUS>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
    I: PinInit<TaskFuture<F>, NTSTATUS>,
{
    let ptr = unsafe { alloc_task::<T, F>() };
    let ptr = match NonNull::new(ptr) {
        Some(ptr) => ptr,
        None => return Err(STATUS_INSUFFICIENT_RESOURCES),
    };

    unsafe {
        core::ptr::addr_of_mut!((*ptr.as_ptr()).header).write(TaskHeader {
            vtable: &FusedTask::<T, F>::VTABLE as *const _ as *mut _,
            ref_count: AtomicU32::new(2),
            status: AtomicU32::new(AsyncStatus::Started.as_raw() | FLAG_POLLING),
            result: UnsafeCell::new(MaybeUninit::uninit()),
        });

        core::ptr::addr_of_mut!((*ptr.as_ptr()).body.dpc).write(core::mem::zeroed());
        if let Err(err) = init.init(core::ptr::addr_of_mut!((*ptr.as_ptr()).body.future)) {
            FusedTask::<T, F>::dealloc(ptr);
            return Err(err);
        }

        KeInitializeDpc(
            &mut (*ptr.as_ptr()).body.dpc as PKDPC,
            Some(FusedTask::<T, F>::dpc_callback),
            ptr.as_ptr() as *mut c_void,
        );
    }

    let poll = unsafe { FusedTask::<T, F>::poll_inner(ptr) };
    match poll {
        Poll::Ready(value) => {
            unsafe { FusedTask::<T, F>::complete(ptr, value) };
            unsafe { FusedTask::<T, F>::release(ptr) };
        }
        Poll::Pending => unsafe { FusedTask::<T, F>::finish_pending(ptr) },
    }

    Ok(ptr.as_ptr() as *mut AsyncOperationRaw<T>)
}

unsafe fn alloc_task<T, F>() -> *mut FusedTask<T, F>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    match FusedTask::<T, F>::BIN_INDEX {
        Some(idx) => unsafe { slab_alloc(idx) as *mut FusedTask<T, F> },
        None => unsafe {
            alloc_aligned(
                wdk_sys::_POOL_TYPE::NonPagedPoolNx as u32,
                core::mem::size_of::<FusedTask<T, F>>(),
                HEAP_TAG,
                core::mem::align_of::<FusedTask<T, F>>(),
            ) as *mut FusedTask<T, F>
        },
    }
}

struct Slab {
    list: UnsafeCell<MaybeUninit<NPAGED_LOOKASIDE_LIST>>,
}

unsafe impl Sync for Slab {}

impl Slab {
    const fn new() -> Self {
        Self {
            list: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    #[inline]
    unsafe fn as_ptr(&self) -> PNPAGED_LOOKASIDE_LIST {
        unsafe { (*self.list.get()).as_mut_ptr() }
    }
}

static SLABS: [Slab; SLAB_COUNT] = [
    Slab::new(),
    Slab::new(),
    Slab::new(),
    Slab::new(),
    Slab::new(),
];

#[doc(hidden)]
/// Initialize fused async COM slab allocators (call at PASSIVE_LEVEL).
pub unsafe fn init_async_com_slabs() {
    ensure_slabs_ready();
}

#[inline]
fn ensure_slabs_ready() {
    let state = SLABS_STATE.load(Ordering::Acquire);
    if state == SLABS_STATE_READY {
        return;
    }

    if state == SLABS_STATE_INITING {
        while SLABS_STATE.load(Ordering::Acquire) != SLABS_STATE_READY {
            core::hint::spin_loop();
        }
        return;
    }

    let irql = unsafe { KeGetCurrentIrql() };
    if irql > PASSIVE_LEVEL as u8 {
        irql_violation();
    }

    if SLABS_STATE
        .compare_exchange(
            SLABS_STATE_UNINIT,
            SLABS_STATE_INITING,
            Ordering::Acquire,
            Ordering::Acquire,
        )
        .is_err()
    {
        while SLABS_STATE.load(Ordering::Acquire) != SLABS_STATE_READY {
            core::hint::spin_loop();
        }
        return;
    }

    for (idx, slab) in SLABS.iter().enumerate() {
        unsafe {
            ExInitializeNPagedLookasideList(
                slab.as_ptr(),
                Some(slab_allocate),
                Some(slab_free_entry),
                0,
                SLAB_SIZES[idx] as SIZE_T,
                SLAB_TAG,
                0,
            );
        }
    }

    SLABS_STATE.store(SLABS_STATE_READY, Ordering::Release);
}

#[inline]
unsafe fn slab_alloc(index: usize) -> *mut u8 {
    ensure_slabs_ready();
    if index >= SLAB_COUNT {
        return core::ptr::null_mut();
    }
    unsafe { ExAllocateFromNPagedLookasideList(SLABS[index].as_ptr()) as *mut u8 }
}

#[inline]
unsafe fn slab_free_indexed(index: usize, ptr: *mut u8) {
    if index >= SLAB_COUNT {
        return;
    }
    unsafe { ExFreeToNPagedLookasideList(SLABS[index].as_ptr(), ptr as PVOID) };
}

unsafe extern "C" fn slab_allocate(pool_type: POOL_TYPE, size: SIZE_T, tag: ULONG) -> PVOID {
    unsafe {
        alloc_aligned(pool_type as u32, size as usize, tag, SLAB_ALIGN) as PVOID
    }
}

unsafe extern "C" fn slab_free_entry(entry: PVOID) {
    unsafe {
        free_aligned(entry as *mut u8, SLAB_TAG);
    }
}

#[inline]
unsafe fn alloc_aligned(pool_type: u32, size: usize, tag: u32, align: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::NonNull::<u8>::dangling().as_ptr();
    }

    let total = match size
        .checked_add(align)
        .and_then(|v| v.checked_add(SLAB_HEADER_SIZE))
    {
        Some(total) => total,
        None => return core::ptr::null_mut(),
    };

    let base = unsafe { ExAllocatePoolWithTag(pool_type, total, tag) } as *mut u8;
    if base.is_null() {
        return core::ptr::null_mut();
    }

    let start = match (base as usize).checked_add(SLAB_HEADER_SIZE) {
        Some(value) => value,
        None => {
            unsafe { ExFreePoolWithTag(base as _, tag) };
            return core::ptr::null_mut();
        }
    };

    let aligned = match start.checked_add(align - 1) {
        Some(value) => value & !(align - 1),
        None => {
            unsafe { ExFreePoolWithTag(base as _, tag) };
            return core::ptr::null_mut();
        }
    };

    let header_ptr = (aligned - SLAB_HEADER_SIZE) as *mut usize;
    unsafe {
        header_ptr.write(base as usize);
    }

    aligned as *mut u8
}

#[inline]
unsafe fn free_aligned(ptr: *mut u8, tag: u32) {
    if ptr.is_null() {
        return;
    }
    let header_ptr = (ptr as usize - SLAB_HEADER_SIZE) as *mut usize;
    let base = unsafe { header_ptr.read() } as *mut u8;
    unsafe { ExFreePoolWithTag(base as _, tag) };
}

const fn select_bin(size: usize, align: usize) -> Option<usize> {
    if align > SLAB_ALIGN {
        return None;
    }
    if size <= SLAB_SIZES[0] {
        return Some(0);
    }
    if size <= SLAB_SIZES[1] {
        return Some(1);
    }
    if size <= SLAB_SIZES[2] {
        return Some(2);
    }
    if size <= SLAB_SIZES[3] {
        return Some(3);
    }
    if size <= SLAB_SIZES[4] {
        return Some(4);
    }
    None
}

#[cold]
#[inline(never)]
fn irql_violation() -> ! {
    #[cfg(debug_assertions)]
    crate::trace::report_error(file!(), line!(), STATUS_UNSUCCESSFUL);

    unsafe {
        crate::ntddk::KeBugCheckEx(0x4B43_4F4D, 0x534C_4142, 0, 0, 0);
    }

    #[allow(unreachable_code)]
    loop {
        core::hint::spin_loop();
    }
}

#[cold]
#[inline(never)]
fn resurrection_violation() -> ! {
    #[cfg(debug_assertions)]
    crate::trace::report_error(file!(), line!(), STATUS_UNSUCCESSFUL);

    unsafe {
        crate::ntddk::KeBugCheckEx(0x4B43_4F4D, 0x5245_5355, 0, 0, 0);
    }

    #[allow(unreachable_code)]
    loop {
        core::hint::spin_loop();
    }
}

unsafe extern "C" {
    fn ExAllocatePoolWithTag(pool_type: u32, number_of_bytes: usize, tag: u32) -> *mut c_void;
    fn ExFreePoolWithTag(p: *mut c_void, tag: u32);
}
