// executor.rs

// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::future::Future;
#[cfg(any(not(feature = "driver"), feature = "async-com-kernel"))]
use core::pin::Pin;
#[cfg(any(not(feature = "driver"), feature = "async-com-kernel"))]
use core::task::{Context, Poll};

use crate::iunknown::{NTSTATUS, STATUS_NOT_SUPPORTED};
#[cfg(any(not(feature = "driver"), feature = "async-com-kernel"))]
use crate::iunknown::STATUS_SUCCESS;

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
use core::task::{RawWaker, RawWakerVTable, Waker};
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
use core::cell::UnsafeCell;

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
use core::ffi::c_void;
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
use core::mem::ManuallyDrop;

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
use core::ptr::{NonNull, null_mut};
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
use crate::ntddk::{
    DEVICE_OBJECT, IoAllocateWorkItem, IoFreeWorkItem, IoQueueWorkItem, ObDereferenceObject,
    ObReferenceObject, PIO_WORKITEM, PIO_WORKITEM_ROUTINE, WORK_QUEUE_TYPE,
};

#[cfg(not(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM")))]
#[allow(non_camel_case_types)]
type DEVICE_OBJECT = core::ffi::c_void;

#[cfg(not(feature = "driver"))]
fn dummy_waker() -> core::task::Waker {
    use core::task::{RawWaker, RawWakerVTable, Waker};

    unsafe fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(core::ptr::null(), &VTABLE)
    }
    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
    unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
use crate::iunknown::STATUS_INSUFFICIENT_RESOURCES;
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
use crate::allocator::{Allocator, KBox, PoolType, WdkAllocator};
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
use crate::refcount;

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
use crate::ntddk::{
    KeAcquireSpinLockRaiseToDpc, KeCancelTimer, KeInitializeDpc, KeInitializeSpinLock,
    KeInitializeTimer, KeInsertQueueDpc, KeReleaseSpinLock, KeRemoveQueueDpc, KeSetTimer, KDPC,
    KIRQL, KSPIN_LOCK, PKDPC, KTIMER, LARGE_INTEGER, PKTIMER,
};

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
type TaskPollFn = for<'a> unsafe fn(*mut TaskHeader, &mut Context<'a>) -> Poll<NTSTATUS>;

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
#[derive(Copy, Clone)]
enum DestroyMode {
    Drop,
    Dealloc,
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
struct TaskVTable {
    poll: TaskPollFn,
    destroy: unsafe fn(*mut TaskHeader, DestroyMode),
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
#[repr(C)]
struct TaskHeader {
    ref_count: AtomicU32,
    scheduled: AtomicU32,
    completed: AtomicU32,
    cancel_requested: AtomicU32,
    dpc: KDPC,
    vtable: &'static TaskVTable,
    alloc_tag: u32,
    tracker: *const TaskTracker,
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
const DEFAULT_TASK_BUDGET: u32 = 64;

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
static DEFAULT_TASK_TAG: AtomicU32 = AtomicU32::new(u32::from_ne_bytes(*b"kcom"));

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
/// Override the default pool tag used by DPC task allocations.
///
/// Call during driver initialization before spawning tasks.
#[inline]
pub fn set_task_alloc_tag(tag: u32) {
    DEFAULT_TASK_TAG.store(tag, Ordering::Release);
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
// NOTE: KeGetCurrentProcessorNumberEx returns a group-relative index.
// MAX_CPU_COUNT=64 is sufficient for typical systems, but processor-group
// configurations may alias indices. Consider dynamic sizing if you need
// perfect mapping on >64 logical CPUs.
const MAX_CPU_COUNT: usize = 64;

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
static CURRENT_TASKS: [AtomicPtr<TaskHeader>; MAX_CPU_COUNT] =
    [const { AtomicPtr::new(null_mut()) }; MAX_CPU_COUNT];

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
#[repr(C)]
#[allow(dead_code, non_camel_case_types, non_snake_case)]
struct PROCESSOR_NUMBER {
    Group: u16,
    Number: u8,
    Reserved: u8,
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
extern "system" {
    fn KeGetCurrentProcessorNumberEx(processor: *mut PROCESSOR_NUMBER);
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
#[inline]
fn current_cpu_index() -> usize {
    let mut processor = PROCESSOR_NUMBER {
        Group: 0,
        Number: 0,
        Reserved: 0,
    };
    unsafe { KeGetCurrentProcessorNumberEx(&mut processor) };
    let index = ((processor.Group as usize) << 6) | (processor.Number as usize);
    index % MAX_CPU_COUNT
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
#[inline]
unsafe fn set_current_task(cpu_index: usize, ptr: NonNull<TaskHeader>) {
    CURRENT_TASKS[cpu_index].store(ptr.as_ptr(), Ordering::Release);
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
#[inline]
unsafe fn clear_current_task(cpu_index: usize) {
    CURRENT_TASKS[cpu_index].store(null_mut(), Ordering::Release);
}

/// Returns true when the currently running DPC task has a cancellation request.
///
/// Only valid inside tasks spawned by the DPC-based executor (spawn_dpc_task*). Work-item
/// tasks may migrate across CPUs, so this helper will not reliably track their state.
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
pub fn is_cancellation_requested() -> bool {
    let irql = unsafe { crate::ntddk::KeGetCurrentIrql() };
    if irql < crate::ntddk::DISPATCH_LEVEL as u8 {
        return false;
    }
    let ptr = CURRENT_TASKS[current_cpu_index()].load(Ordering::Acquire);
    if ptr.is_null() {
        return false;
    }

    unsafe { (*ptr).cancel_requested.load(Ordering::Relaxed) != 0 }
}

/// Stub for non-kernel builds.
#[cfg(not(all(feature = "driver", feature = "async-com-kernel")))]
pub fn is_cancellation_requested() -> bool {
    false
}

/// Returns true only once per cancellation request, then marks it as acknowledged.
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
#[inline]
pub(crate) fn take_cancellation_request() -> bool {
    let irql = unsafe { crate::ntddk::KeGetCurrentIrql() };
    if irql < crate::ntddk::DISPATCH_LEVEL as u8 {
        return false;
    }
    let ptr = CURRENT_TASKS[current_cpu_index()].load(Ordering::Acquire);
    if ptr.is_null() {
        return false;
    }

    let header = unsafe { &*ptr };
    header
        .cancel_requested
        .compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

/// Stub for non-kernel builds.
#[cfg(not(all(feature = "driver", feature = "async-com-kernel")))]
#[inline]
pub(crate) fn take_cancellation_request() -> bool {
    false
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl TaskHeader {
    #[inline]
    unsafe fn add_ref(ptr: NonNull<Self>) {
        let header = unsafe { &*ptr.as_ptr() };
        let _ = refcount::add(&header.ref_count);
    }

    unsafe fn release(ptr: NonNull<Self>) {
        let header = unsafe { &*ptr.as_ptr() };
        let count = refcount::sub(&header.ref_count);
        if count != 0 {
            return;
        }

        core::sync::atomic::fence(Ordering::Acquire);
        let header = unsafe { &*ptr.as_ptr() };
        let tracker = header.tracker;
        if header.completed.load(Ordering::Acquire) == 0 {
            header.completed.store(1, Ordering::Release);
            unsafe { (header.vtable.destroy)(ptr.as_ptr(), DestroyMode::Drop) };
            unsafe { (header.vtable.destroy)(ptr.as_ptr(), DestroyMode::Dealloc) };
            unsafe { task_tracker_complete(tracker) };
            return;
        }

        unsafe { (header.vtable.destroy)(ptr.as_ptr(), DestroyMode::Dealloc) };
        unsafe { task_tracker_complete(tracker) };
    }

    #[inline]
    unsafe fn cancel(ptr: NonNull<Self>) {
        let header = unsafe { &*ptr.as_ptr() };
        if header
            .cancel_requested
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::Acquire)
            .is_ok()
        {
            unsafe { Self::schedule(ptr) };
        }
    }

    #[inline]
    unsafe fn schedule(ptr: NonNull<Self>) {
        if unsafe { &*ptr.as_ptr() }
            .completed
            .load(Ordering::Acquire)
            != 0
        {
            return;
        }

        if unsafe { &*ptr.as_ptr() }
            .scheduled
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        unsafe { Self::add_ref(ptr) };
        let inserted = unsafe {
            KeInsertQueueDpc(
                &mut (*ptr.as_ptr()).dpc as PKDPC,
                null_mut(),
                null_mut(),
            )
        };

        if inserted == 0 {
            unsafe { Self::release(ptr) };
        }
    }

    #[inline]
    unsafe fn queue_dpc(ptr: NonNull<Self>) {
        unsafe { Self::add_ref(ptr) };
        let inserted = unsafe {
            KeInsertQueueDpc(
                &mut (*ptr.as_ptr()).dpc as PKDPC,
                null_mut(),
                null_mut(),
            )
        };

        if inserted == 0 {
            unsafe { Self::release(ptr) };
        }
    }

    unsafe extern "C" fn dpc_routine(
        _dpc: PKDPC,
        deferred_context: *mut c_void,
        _system_argument1: *mut c_void,
        _system_argument2: *mut c_void,
    ) {
        let ptr = match NonNull::new(deferred_context as *mut TaskHeader) {
            Some(p) => p,
            None => return,
        };

        let cpu_index = current_cpu_index();
        unsafe { &*ptr.as_ptr() }.scheduled.store(0, Ordering::Release);

        if unsafe { &*ptr.as_ptr() }
            .completed
            .load(Ordering::Acquire)
            != 0
        {
            unsafe { Self::release(ptr) };
            return;
        }

        let waker = unsafe { Waker::from_raw(Self::raw_waker(ptr)) };
        let mut cx = Context::from_waker(&waker);

        unsafe { set_current_task(cpu_index, ptr) };
        let mut budget = DEFAULT_TASK_BUDGET;

        loop {
            let poll = unsafe { ((*ptr.as_ptr()).vtable.poll)(ptr.as_ptr(), &mut cx) };
            match poll {
                Poll::Ready(_status) => {
                    unsafe { &*ptr.as_ptr() }.completed.store(1, Ordering::Release);
                    unsafe {
                        ((*ptr.as_ptr()).vtable.destroy)(ptr.as_ptr(), DestroyMode::Drop)
                    };
                    unsafe { clear_current_task(cpu_index) };
                    unsafe { Self::release(ptr) };
                    return;
                }
                Poll::Pending => {
                    let woken = unsafe { &*ptr.as_ptr() }
                        .scheduled
                        .load(Ordering::Acquire)
                        != 0;
                    if !woken {
                        break;
                    }

                    if budget == 0 {
                        unsafe { Self::queue_dpc(ptr) };
                        break;
                    }

                    unsafe { &*ptr.as_ptr() }
                        .scheduled
                        .store(0, Ordering::Release);
                    budget -= 1;
                }
            }
        }

        unsafe { clear_current_task(cpu_index) };
        unsafe { Self::release(ptr) };
    }

    #[inline]
    unsafe fn raw_waker(ptr: NonNull<Self>) -> RawWaker {
        unsafe { Self::add_ref(ptr) };
        RawWaker::new(ptr.as_ptr() as *const (), &Self::RAW_WAKER_VTABLE)
    }

    const RAW_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        Self::clone_raw,
        Self::wake_raw,
        Self::wake_by_ref_raw,
        Self::drop_raw,
    );

    unsafe fn clone_raw(ptr: *const ()) -> RawWaker {
        let ptr = match NonNull::new(ptr as *mut TaskHeader) {
            Some(p) => p,
            None => return RawWaker::new(null_mut(), &Self::RAW_WAKER_VTABLE),
        };
        unsafe { Self::raw_waker(ptr) }
    }

    unsafe fn wake_raw(ptr: *const ()) {
        let ptr = match NonNull::new(ptr as *mut TaskHeader) {
            Some(p) => p,
            None => return,
        };
        unsafe { Self::schedule(ptr) };
        unsafe { Self::release(ptr) };
    }

    unsafe fn wake_by_ref_raw(ptr: *const ()) {
        let ptr = match NonNull::new(ptr as *mut TaskHeader) {
            Some(p) => p,
            None => return,
        };
        unsafe { Self::schedule(ptr) };
    }

    unsafe fn drop_raw(ptr: *const ()) {
        let ptr = match NonNull::new(ptr as *mut TaskHeader) {
            Some(p) => p,
            None => return,
        };
        unsafe { Self::release(ptr) };
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
#[repr(C)]
struct Task<F>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    header: TaskHeader,
    future: ManuallyDrop<F>,
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl<F> Task<F>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    #[inline]
    fn alloc_tag() -> u32 {
        DEFAULT_TASK_TAG.load(Ordering::Acquire)
    }

    const VTABLE: TaskVTable = TaskVTable {
        poll: Self::poll_shim,
        destroy: Self::destroy_shim,
    };

    unsafe fn allocate(
        future: F,
        tracker: *const TaskTracker,
    ) -> Result<NonNull<TaskHeader>, NTSTATUS> {
        let tag = Self::alloc_tag();
        let alloc = WdkAllocator::new(PoolType::NonPagedNx, tag);
        let layout = core::alloc::Layout::new::<Task<F>>();

        let ptr = unsafe { alloc.alloc(layout) } as *mut Task<F>;
        let ptr = NonNull::new(ptr).ok_or(STATUS_INSUFFICIENT_RESOURCES)?;

        unsafe {
            core::ptr::write(
                ptr.as_ptr(),
                Task {
                    header: TaskHeader {
                        ref_count: AtomicU32::new(1),
                        scheduled: AtomicU32::new(0),
                        completed: AtomicU32::new(0),
                        cancel_requested: AtomicU32::new(0),
                        dpc: core::mem::zeroed(),
                        vtable: &Self::VTABLE,
                        alloc_tag: tag,
                        tracker,
                    },
                    future: ManuallyDrop::new(future),
                },
            );

            KeInitializeDpc(
                &mut (*ptr.as_ptr()).header.dpc as PKDPC,
                Some(TaskHeader::dpc_routine),
                &mut (*ptr.as_ptr()).header as *mut TaskHeader as *mut c_void,
            );
        }

        unsafe { task_tracker_begin(tracker) };

        Ok(unsafe { NonNull::new_unchecked(&mut (*ptr.as_ptr()).header) })
    }

    unsafe fn poll_shim(header: *mut TaskHeader, cx: &mut Context<'_>) -> Poll<NTSTATUS> {
        let task = header as *mut Task<F>;
        let fut = unsafe { &mut *(*task).future };
        let fut = unsafe { Pin::new_unchecked(fut) };
        fut.poll(cx)
    }

    unsafe fn destroy_shim(header: *mut TaskHeader, mode: DestroyMode) {
        let task = header as *mut Task<F>;
        match mode {
            DestroyMode::Drop => unsafe { ManuallyDrop::drop(&mut (*task).future) },
            DestroyMode::Dealloc => {
                let tag = unsafe { (*task).header.alloc_tag };
                let alloc = WdkAllocator::new(PoolType::NonPagedNx, tag);
                let layout = core::alloc::Layout::new::<Task<F>>();
                unsafe {
                    let task = NonNull::new_unchecked(task);
                    drop(KBox::from_raw_parts(task, alloc, layout));
                }
            }
        }
    }
}

/// Handle for requesting cancellation on a spawned task.
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
pub struct CancelHandle {
    task: AtomicPtr<TaskHeader>,
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
unsafe impl Send for CancelHandle {}
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
unsafe impl Sync for CancelHandle {}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl CancelHandle {
    #[inline]
    unsafe fn new(ptr: NonNull<TaskHeader>) -> Self {
        unsafe { TaskHeader::add_ref(ptr) };
        Self {
            task: AtomicPtr::new(ptr.as_ptr()),
        }
    }

    /// Request cancellation for the associated task.
    #[inline]
    pub fn cancel(&self) {
        let ptr = self.task.load(Ordering::Acquire);
        let Some(ptr) = NonNull::new(ptr) else {
            return;
        };

        unsafe { TaskHeader::cancel(ptr) };
    }

    /// Check whether cancellation has been requested.
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        let ptr = self.task.load(Ordering::Acquire);
        let Some(ptr) = NonNull::new(ptr) else {
            return false;
        };

        unsafe { (*ptr.as_ptr()).cancel_requested.load(Ordering::Relaxed) != 0 }
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl Drop for CancelHandle {
    fn drop(&mut self) {
        let ptr = self.task.swap(null_mut(), Ordering::AcqRel);
        if let Some(ptr) = NonNull::new(ptr) {
            unsafe { TaskHeader::release(ptr) };
        }
    }
}

/// Stub handle for non-kernel builds.
#[cfg(not(all(feature = "driver", feature = "async-com-kernel")))]
pub struct CancelHandle;

#[cfg(not(all(feature = "driver", feature = "async-com-kernel")))]
impl CancelHandle {
    #[inline]
    pub fn cancel(&self) {}

    #[inline]
    pub fn is_cancelled(&self) -> bool {
        false
    }
}

#[cfg(not(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM")))]
pub struct WorkItemTracker;

#[cfg(not(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM")))]
impl WorkItemTracker {
    #[inline]
    pub fn new() -> Self {
        Self
    }

    #[inline]
    pub fn drain(&self) {}
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
struct SpinLock<T> {
    lock: UnsafeCell<KSPIN_LOCK>,
    value: UnsafeCell<T>,
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
unsafe impl<T: Send> Send for SpinLock<T> {}
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
unsafe impl<T: Send> Sync for SpinLock<T> {}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl<T> SpinLock<T> {
    #[inline]
    fn new(value: T) -> Self {
        let mut lock = unsafe { core::mem::zeroed() };
        unsafe { KeInitializeSpinLock(&mut lock) };
        Self {
            lock: UnsafeCell::new(lock),
            value: UnsafeCell::new(value),
        }
    }

    #[inline]
    fn lock(&self) -> SpinLockGuard<'_, T> {
        let old_irql = unsafe { KeAcquireSpinLockRaiseToDpc(self.lock.get()) };
        SpinLockGuard {
            lock: self,
            old_irql,
        }
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
    old_irql: KIRQL,
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl<T> core::ops::Deref for SpinLockGuard<'_, T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.value.get() }
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl<T> core::ops::DerefMut for SpinLockGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.value.get() }
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl<T> Drop for SpinLockGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        unsafe { KeReleaseSpinLock(self.lock.lock.get(), self.old_irql) };
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
struct KernelTimerInner {
    ref_count: AtomicU32,
    fired: AtomicU32,
    armed: AtomicU32,
    cancelled: AtomicU32,
    timer: KTIMER,
    dpc: KDPC,
    waker: SpinLock<Option<Waker>>,
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl KernelTimerInner {
    unsafe fn allocate() -> Result<NonNull<Self>, NTSTATUS> {
        let alloc = WdkAllocator::new(PoolType::NonPagedNx, u32::from_ne_bytes(*b"irnt"));

        let layout = core::alloc::Layout::new::<KernelTimerInner>();

        let ptr = unsafe { alloc.alloc(layout) } as *mut KernelTimerInner;
        let ptr = NonNull::new(ptr).ok_or(STATUS_INSUFFICIENT_RESOURCES)?;

        unsafe {
            core::ptr::write(
                ptr.as_ptr(),
                KernelTimerInner {
                    ref_count: AtomicU32::new(1),
                    fired: AtomicU32::new(0),
                    armed: AtomicU32::new(0),
                    cancelled: AtomicU32::new(0),
                    timer: core::mem::zeroed(),
                    dpc: core::mem::zeroed(),
                    waker: SpinLock::new(None),
                },
            );
        }

        Ok(ptr)
    }

    #[inline]
    unsafe fn add_ref(ptr: NonNull<Self>) {
        let inner = unsafe { &*ptr.as_ptr() };
        let _ = refcount::add(&inner.ref_count);
    }

    unsafe fn release(ptr: NonNull<Self>) {
        let inner = unsafe { &*ptr.as_ptr() };
        let count = refcount::sub(&inner.ref_count);
        if count != 0 {
            return;
        }

        core::sync::atomic::fence(Ordering::Acquire);
        unsafe { Self::free(ptr) }
    }

    unsafe fn free(ptr: NonNull<Self>) {
        let alloc = WdkAllocator::new(PoolType::NonPagedNx, u32::from_ne_bytes(*b"irnt"));
        let layout = core::alloc::Layout::new::<KernelTimerInner>();
        unsafe { drop(KBox::from_raw_parts(ptr, alloc, layout)) }
    }
}

/// A timer-based future for kernel mode.
///
/// `due_time_100ns` must be a relative negative interval in 100ns units
/// (i.e., like the `DueTime` passed to `KeSetTimer`).
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
pub struct KernelTimerFuture {
    inner: NonNull<KernelTimerInner>,
    due_time_100ns: i64,
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
unsafe impl Send for KernelTimerFuture {}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl KernelTimerFuture {
    #[inline]
    pub fn new(due_time_100ns: i64) -> Result<Self, NTSTATUS> {
        let inner = unsafe { KernelTimerInner::allocate() }?;
        Ok(Self { inner, due_time_100ns })
    }

    unsafe extern "C" fn timer_dpc_routine(
        _dpc: PKDPC,
        deferred_context: *mut c_void,
        _system_argument1: *mut c_void,
        _system_argument2: *mut c_void,
    ) {
        let this = match NonNull::new(deferred_context as *mut KernelTimerInner) {
            Some(p) => p,
            None => return,
        };

        unsafe { &*this.as_ptr() }.fired.store(1, Ordering::Release);

        if unsafe { &*this.as_ptr() }.cancelled.load(Ordering::Acquire) == 0 {
            let guard = unsafe { &*this.as_ptr() }.waker.lock();

            if let Some(w) = guard.as_ref() {
                w.wake_by_ref();
            }
        }

        unsafe { KernelTimerInner::release(this) };
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl Future for KernelTimerFuture {
    type Output = NTSTATUS;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let inner = unsafe { &*this.inner.as_ptr() };

        if inner.fired.load(Ordering::Acquire) != 0 {
            return Poll::Ready(STATUS_SUCCESS);
        }

        {
            let mut guard = inner.waker.lock();
            *guard = Some(cx.waker().clone());
        }

        if inner
            .armed
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            unsafe {
                KernelTimerInner::add_ref(this.inner);
                KeInitializeTimer(&mut (*this.inner.as_ptr()).timer as PKTIMER);
                KeInitializeDpc(
                    &mut (*this.inner.as_ptr()).dpc as PKDPC,
                    Some(Self::timer_dpc_routine),
                    this.inner.as_ptr() as *mut c_void,
                );

                let due = LARGE_INTEGER {
                    QuadPart: this.due_time_100ns,
                };
                let _ = KeSetTimer(
                    &mut (*this.inner.as_ptr()).timer as PKTIMER,
                    due,
                    &mut (*this.inner.as_ptr()).dpc as PKDPC,
                );
            }
        }
        Poll::Pending
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl Drop for KernelTimerFuture {
    fn drop(&mut self) {
        unsafe {
            let inner = self.inner;
            (*inner.as_ptr()).cancelled.store(1, Ordering::Release);
            let cancelled = KeCancelTimer(&mut (*inner.as_ptr()).timer as PKTIMER);
            let _ = KeRemoveQueueDpc(&mut (*inner.as_ptr()).dpc as PKDPC);
            KernelTimerInner::release(inner);
            if cancelled != 0 {
                KernelTimerInner::release(inner);
            }
        }
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
struct WorkItemTask<F>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    ref_count: AtomicU32,
    scheduled: AtomicU32,
    completed: AtomicU32,
    cancel_requested: AtomicU32,
    future: ManuallyDrop<F>,
    device: *mut DEVICE_OBJECT,
    tracker: *const WorkItemTracker,
    work_item: AtomicPtr<PIO_WORKITEM>,
}

/// Handle for requesting cancellation on a work-item task.
#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
pub struct WorkItemCancelHandle<F>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    task: AtomicPtr<WorkItemTask<F>>,
}

#[cfg(not(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM")))]
pub struct WorkItemCancelHandle<F>(core::marker::PhantomData<F>);

#[cfg(not(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM")))]
impl<F> WorkItemCancelHandle<F> {
    #[inline]
    pub fn cancel(&self) {}

    #[inline]
    pub fn is_cancelled(&self) -> bool {
        false
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
unsafe impl<F> Send for WorkItemCancelHandle<F> where F: Future<Output = NTSTATUS> + Send + 'static {}
#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
unsafe impl<F> Sync for WorkItemCancelHandle<F> where F: Future<Output = NTSTATUS> + Send + 'static {}

#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
impl<F> WorkItemCancelHandle<F>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    #[inline]
    unsafe fn new(ptr: NonNull<WorkItemTask<F>>) -> Self {
        unsafe { WorkItemTask::<F>::add_ref(ptr) };
        Self {
            task: AtomicPtr::new(ptr.as_ptr()),
        }
    }

    /// Request cancellation for the associated task.
    ///
    /// Cancellation queues a work item to drop the future, so call at PASSIVE_LEVEL.
    #[inline]
    pub fn cancel(&self) {
        let ptr = self.task.load(Ordering::Acquire);
        let Some(ptr) = NonNull::new(ptr) else {
            return;
        };

        unsafe { WorkItemTask::<F>::cancel(ptr) };
    }

    /// Check whether cancellation has been requested.
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        let ptr = self.task.load(Ordering::Acquire);
        let Some(ptr) = NonNull::new(ptr) else {
            return false;
        };

        unsafe { (*ptr.as_ptr()).cancel_requested.load(Ordering::Relaxed) != 0 }
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
impl<F> Clone for WorkItemCancelHandle<F>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    fn clone(&self) -> Self {
        let ptr = self.task.load(Ordering::Acquire);
        if let Some(ptr) = NonNull::new(ptr) {
            unsafe { WorkItemTask::<F>::add_ref(ptr) };
        }
        Self {
            task: AtomicPtr::new(ptr),
        }
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
impl<F> Drop for WorkItemCancelHandle<F>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    fn drop(&mut self) {
        let ptr = self.task.swap(null_mut(), Ordering::AcqRel);
        if let Some(ptr) = NonNull::new(ptr) {
            unsafe { WorkItemTask::<F>::release(ptr) };
        }
    }
}

/// Tracks outstanding work items so you can drain them before driver unload.
#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
pub struct WorkItemTracker {
    pending: AtomicU32,
    event: UnsafeCell<crate::ntddk::KEVENT>,
}

#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
unsafe impl Send for WorkItemTracker {}
#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
unsafe impl Sync for WorkItemTracker {}

#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
impl WorkItemTracker {
    #[inline]
    pub fn new() -> Self {
        let mut event = unsafe { core::mem::zeroed() };
        unsafe {
            crate::ntddk::KeInitializeEvent(
                &mut event,
                crate::ntddk::SynchronizationEvent,
                1,
            );
        }
        Self {
            pending: AtomicU32::new(0),
            event: UnsafeCell::new(event),
        }
    }

    #[inline]
    fn begin(&self) {
        let prev = self.pending.fetch_add(1, Ordering::AcqRel);
        if prev == 0 {
            unsafe {
                crate::ntddk::KeInitializeEvent(
                    self.event.get(),
                    crate::ntddk::SynchronizationEvent,
                    0,
                );
            }
        }
    }

    #[inline]
    fn complete(&self) {
        let prev = self.pending.fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            unsafe {
                crate::ntddk::KeSetEvent(self.event.get(), 0, 0);
            }
        }
    }

    /// Wait until all tracked work items have completed.
    ///
    /// Call only after you stop submitting new work items.
    #[inline]
    pub fn drain(&self) {
        if self.pending.load(Ordering::Acquire) == 0 {
            return;
        }

        unsafe {
            let _ = crate::ntddk::KeWaitForSingleObject(
                self.event.get() as *mut c_void,
                crate::ntddk::_KWAIT_REASON::Executive,
                crate::ntddk::_MODE::KernelMode as i8,
                0,
                core::ptr::null_mut(),
            );
        }
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
#[inline]
unsafe fn tracker_begin(tracker: *const WorkItemTracker) {
    if !tracker.is_null() {
        unsafe { (*tracker).begin() };
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
#[inline]
unsafe fn tracker_complete(tracker: *const WorkItemTracker) {
    if !tracker.is_null() {
        unsafe { (*tracker).complete() };
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
impl<F> WorkItemTask<F>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    #[inline]
    fn alloc_tag() -> u32 {
        u32::from_ne_bytes(*b"kcow")
    }

    unsafe fn allocate(future: F) -> Result<NonNull<Self>, NTSTATUS> {
        let alloc = WdkAllocator::new(PoolType::NonPagedNx, Self::alloc_tag());
        let layout = core::alloc::Layout::new::<WorkItemTask<F>>();

        let ptr = unsafe { alloc.alloc(layout) } as *mut WorkItemTask<F>;
        let ptr = NonNull::new(ptr).ok_or(STATUS_INSUFFICIENT_RESOURCES)?;

        unsafe {
            core::ptr::write(
                ptr.as_ptr(),
                WorkItemTask {
                    ref_count: AtomicU32::new(1),
                    scheduled: AtomicU32::new(0),
                    completed: AtomicU32::new(0),
                    cancel_requested: AtomicU32::new(0),
                    future: ManuallyDrop::new(future),
                    device: null_mut(),
                    tracker: core::ptr::null(),
                    work_item: AtomicPtr::new(null_mut()),
                },
            );
        }

        Ok(ptr)
    }

    #[inline]
    unsafe fn add_ref(ptr: NonNull<Self>) {
        let task = unsafe { &*ptr.as_ptr() };
        let _ = refcount::add(&task.ref_count);
    }

    unsafe fn release(ptr: NonNull<Self>) {
        let task = unsafe { &*ptr.as_ptr() };
        let count = refcount::sub(&task.ref_count);
        if count != 0 {
            return;
        }

        core::sync::atomic::fence(Ordering::Acquire);
        unsafe { Self::free(ptr) }
    }

    unsafe fn free(ptr: NonNull<Self>) {
        let alloc = WdkAllocator::new(PoolType::NonPagedNx, Self::alloc_tag());
        let device = unsafe { (*ptr.as_ptr()).device };
        unsafe {
            drop(KBox::from_raw_parts(
                ptr,
                alloc,
                core::alloc::Layout::new::<WorkItemTask<F>>(),
            ));
        }
        if !device.is_null() {
            unsafe { ObDereferenceObject(device.cast()) };
        }
    }

    unsafe fn schedule(ptr: NonNull<Self>) -> NTSTATUS {
        if unsafe { &*ptr.as_ptr() }
            .completed
            .load(Ordering::Acquire)
            != 0
        {
            return STATUS_SUCCESS;
        }

        if unsafe { &*ptr.as_ptr() }
            .scheduled
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return STATUS_SUCCESS;
        }

        let tracker = unsafe { &*ptr.as_ptr() }.tracker;
        unsafe { tracker_begin(tracker) };

        let device = unsafe { &*ptr.as_ptr() }.device;
        if device.is_null() {
            unsafe { &*ptr.as_ptr() }.scheduled.store(0, Ordering::Release);
            unsafe { tracker_complete(tracker) };
            return STATUS_NOT_SUPPORTED;
        }

        let work_item = unsafe { IoAllocateWorkItem(device) };
        if work_item.is_null() {
            unsafe { &*ptr.as_ptr() }.scheduled.store(0, Ordering::Release);
            unsafe { tracker_complete(tracker) };
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        unsafe { Self::add_ref(ptr) };
        unsafe { &*ptr.as_ptr() }
            .work_item
            .store(work_item, Ordering::Release);

        unsafe {
            IoQueueWorkItem(
                work_item,
                Some(Self::work_item_routine as PIO_WORKITEM_ROUTINE),
                WORK_QUEUE_TYPE::DelayedWorkQueue,
                ptr.as_ptr() as *mut c_void,
            );
        }

        STATUS_SUCCESS
    }

    #[inline]
    unsafe fn cancel(ptr: NonNull<Self>) {
        let header = unsafe { &*ptr.as_ptr() };
        if header
            .cancel_requested
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::Acquire)
            .is_ok()
        {
            let _ = unsafe { Self::schedule(ptr) };
        }
    }

    unsafe extern "C" fn work_item_routine(
        _device: *mut DEVICE_OBJECT,
        context: *mut c_void,
    ) {
        let ptr = match NonNull::new(context as *mut WorkItemTask<F>) {
            Some(p) => p,
            None => return,
        };

        let tracker = unsafe { &*ptr.as_ptr() }.tracker;

        unsafe { &*ptr.as_ptr() }.scheduled.store(0, Ordering::Release);

        if unsafe { &*ptr.as_ptr() }
            .completed
            .load(Ordering::Acquire)
            != 0
        {
            unsafe { tracker_complete(tracker) };
            unsafe { Self::release(ptr) };
            return;
        }

        let cancelled = unsafe { &*ptr.as_ptr() }
            .cancel_requested
            .load(Ordering::Acquire)
            != 0;
        if cancelled {
            unsafe { &*ptr.as_ptr() }.completed.store(1, Ordering::Release);
            unsafe { ManuallyDrop::drop(&mut (*ptr.as_ptr()).future) };
        } else {
            let waker = unsafe { Waker::from_raw(Self::raw_waker(ptr)) };
            let mut cx = Context::from_waker(&waker);

            let poll = unsafe {
                let task = &mut *ptr.as_ptr();
                let fut = Pin::new_unchecked(&mut *task.future);
                fut.poll(&mut cx)
            };

            if let Poll::Ready(_status) = poll {
                unsafe { &*ptr.as_ptr() }.completed.store(1, Ordering::Release);
                unsafe { ManuallyDrop::drop(&mut (*ptr.as_ptr()).future) };
            }
        }

        let work_item = unsafe { &*ptr.as_ptr() }
            .work_item
            .swap(null_mut(), Ordering::AcqRel);
        if !work_item.is_null() {
            unsafe { IoFreeWorkItem(work_item) };
        }

        unsafe { tracker_complete(tracker) };
        unsafe { Self::release(ptr) };
    }

    #[inline]
    unsafe fn raw_waker(ptr: NonNull<Self>) -> RawWaker {
        unsafe { Self::add_ref(ptr) };
        RawWaker::new(ptr.as_ptr() as *const (), &Self::RAW_WAKER_VTABLE)
    }

    const RAW_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        Self::clone_raw,
        Self::wake_raw,
        Self::wake_by_ref_raw,
        Self::drop_raw,
    );

    unsafe fn clone_raw(ptr: *const ()) -> RawWaker {
        let ptr = match NonNull::new(ptr as *mut WorkItemTask<F>) {
            Some(p) => p,
            None => return RawWaker::new(core::ptr::null(), &Self::RAW_WAKER_VTABLE),
        };
        unsafe { Self::raw_waker(ptr) }
    }

    unsafe fn wake_raw(ptr: *const ()) {
        let ptr = match NonNull::new(ptr as *mut WorkItemTask<F>) {
            Some(p) => p,
            None => return,
        };
        let _ = unsafe { Self::schedule(ptr) };
        unsafe { Self::release(ptr) };
    }

    unsafe fn wake_by_ref_raw(ptr: *const ()) {
        let ptr = match NonNull::new(ptr as *mut WorkItemTask<F>) {
            Some(p) => p,
            None => return,
        };
        let _ = unsafe { Self::schedule(ptr) };
    }

    unsafe fn drop_raw(ptr: *const ()) {
        let ptr = match NonNull::new(ptr as *mut WorkItemTask<F>) {
            Some(p) => p,
            None => return,
        };
        unsafe { Self::release(ptr) };
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
unsafe impl<F> Send for WorkItemTask<F> where F: Future<Output = NTSTATUS> + Send + 'static {}
#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
unsafe impl<F> Sync for WorkItemTask<F> where F: Future<Output = NTSTATUS> + Send + 'static {}

/// Spawn a future onto the PASSIVE_LEVEL work-item executor (WDM).
///
/// Note: ensure outstanding work items are drained before driver unload to
/// avoid freeing device objects while work-item callbacks are still running.
#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
pub fn spawn_task<F>(device: *mut DEVICE_OBJECT, future: F) -> NTSTATUS
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    let ptr = match unsafe { WorkItemTask::<F>::allocate(future) } {
        Ok(p) => p,
        Err(s) => return s,
    };

    unsafe {
        if !device.is_null() {
            ObReferenceObject(device.cast());
        }
        (&mut *ptr.as_ptr()).device = device;
    }
    let status = unsafe { WorkItemTask::<F>::schedule(ptr) };
    unsafe { WorkItemTask::<F>::release(ptr) };

    status
}

/// Spawn a future onto the PASSIVE_LEVEL work-item executor (WDM), tracking
/// outstanding work so you can drain before unload.
#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
pub fn spawn_task_tracked<F>(
    device: *mut DEVICE_OBJECT,
    tracker: &WorkItemTracker,
    future: F,
) -> NTSTATUS
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    let ptr = match unsafe { WorkItemTask::<F>::allocate(future) } {
        Ok(p) => p,
        Err(s) => return s,
    };

    unsafe {
        let task = &mut *ptr.as_ptr();
        if !device.is_null() {
            ObReferenceObject(device.cast());
        }
        task.device = device;
        task.tracker = tracker as *const WorkItemTracker;
    }
    let status = unsafe { WorkItemTask::<F>::schedule(ptr) };
    unsafe { WorkItemTask::<F>::release(ptr) };

    status
}

/// Spawn a future onto the PASSIVE_LEVEL work-item executor (WDM) and return a
/// cancellation handle.
#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
pub fn spawn_task_cancellable<F>(
    device: *mut DEVICE_OBJECT,
    future: F,
) -> Result<WorkItemCancelHandle<F>, NTSTATUS>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    let ptr = match unsafe { WorkItemTask::<F>::allocate(future) } {
        Ok(p) => p,
        Err(s) => return Err(s),
    };

    unsafe {
        let task = &mut *ptr.as_ptr();
        if !device.is_null() {
            ObReferenceObject(device.cast());
        }
        task.device = device;
        task.tracker = core::ptr::null();
    }

    let handle = unsafe { WorkItemCancelHandle::new(ptr) };
    let status = unsafe { WorkItemTask::<F>::schedule(ptr) };
    unsafe { WorkItemTask::<F>::release(ptr) };

    if status != STATUS_SUCCESS {
        drop(handle);
        return Err(status);
    }

    Ok(handle)
}

/// Spawn a future onto the kcom DPC executor (driver build without async-com-kernel).
#[cfg(all(feature = "driver", not(feature = "async-com-kernel")))]
pub unsafe fn spawn_dpc_task_cancellable<F>(_future: F) -> Result<CancelHandle, NTSTATUS>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    Err(STATUS_NOT_SUPPORTED)
}

/// Spawn a future onto the kcom DPC executor (host stub).
#[cfg(not(feature = "driver"))]
pub unsafe fn spawn_dpc_task_cancellable<F>(mut future: F) -> Result<CancelHandle, NTSTATUS>
where
    F: Future<Output = NTSTATUS> + 'static,
{
    let waker = dummy_waker();

    let mut cx = Context::from_waker(&waker);
    let mut future = unsafe { Pin::new_unchecked(&mut future) };
    let _ = future.as_mut().poll(&mut cx);
    Ok(CancelHandle)
}

/// Spawn a future onto the PASSIVE_LEVEL work-item executor (WDM), tracking
/// outstanding work and returning a cancellation handle.
#[cfg(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
pub fn spawn_task_cancellable_tracked<F>(
    device: *mut DEVICE_OBJECT,
    tracker: &WorkItemTracker,
    future: F,
) -> Result<WorkItemCancelHandle<F>, NTSTATUS>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    let ptr = match unsafe { WorkItemTask::<F>::allocate(future) } {
        Ok(p) => p,
        Err(s) => return Err(s),
    };

    unsafe {
        let task = &mut *ptr.as_ptr();
        if !device.is_null() {
            ObReferenceObject(device.cast());
        }
        task.device = device;
        task.tracker = tracker as *const WorkItemTracker;
    }

    let handle = unsafe { WorkItemCancelHandle::new(ptr) };
    let status = unsafe { WorkItemTask::<F>::schedule(ptr) };
    unsafe { WorkItemTask::<F>::release(ptr) };

    if status != STATUS_SUCCESS {
        drop(handle);
        return Err(status);
    }

    Ok(handle)
}

/// Spawn a future onto the PASSIVE_LEVEL work-item executor (unsupported builds).
#[cfg(not(all(feature = "driver", feature = "async-com-kernel", driver_model__driver_type = "WDM")))]
pub fn spawn_task_cancellable_tracked<F>(
    _device: *mut DEVICE_OBJECT,
    _tracker: &WorkItemTracker,
    _future: F,
) -> Result<WorkItemCancelHandle<F>, NTSTATUS>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    Err(STATUS_NOT_SUPPORTED)
}

/// Tracks outstanding DPC tasks so you can drain them before driver unload.
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
pub struct TaskTracker {
    pending: AtomicU32,
    event: UnsafeCell<crate::ntddk::KEVENT>,
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
unsafe impl Send for TaskTracker {}
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
unsafe impl Sync for TaskTracker {}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
impl TaskTracker {
    #[inline]
    pub fn new() -> Self {
        let mut event = unsafe { core::mem::zeroed() };
        unsafe {
            crate::ntddk::KeInitializeEvent(
                &mut event,
                crate::ntddk::SynchronizationEvent,
                1,
            );
        }
        Self {
            pending: AtomicU32::new(0),
            event: UnsafeCell::new(event),
        }
    }

    #[inline]
    fn begin(&self) {
        let prev = self.pending.fetch_add(1, Ordering::AcqRel);
        if prev == 0 {
            unsafe {
                crate::ntddk::KeInitializeEvent(
                    self.event.get(),
                    crate::ntddk::SynchronizationEvent,
                    0,
                );
            }
        }
    }

    #[inline]
    fn complete(&self) {
        let prev = self.pending.fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            unsafe {
                crate::ntddk::KeSetEvent(self.event.get(), 0, 0);
            }
        }
    }

    /// Wait until all tracked DPC tasks have completed.
    ///
    /// Call only after you stop submitting new tasks.
    #[inline]
    pub fn drain(&self) {
        if self.pending.load(Ordering::Acquire) == 0 {
            return;
        }

        unsafe {
            let _ = crate::ntddk::KeWaitForSingleObject(
                self.event.get() as *mut c_void,
                crate::ntddk::_KWAIT_REASON::Executive,
                crate::ntddk::_MODE::KernelMode as i8,
                0,
                core::ptr::null_mut(),
            );
        }
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
#[inline]
unsafe fn task_tracker_begin(tracker: *const TaskTracker) {
    if !tracker.is_null() {
        unsafe { (*tracker).begin() };
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
#[inline]
unsafe fn task_tracker_complete(tracker: *const TaskTracker) {
    if !tracker.is_null() {
        unsafe { (*tracker).complete() };
    }
}

#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
/// Spawn a future onto the kcom DPC executor and return a cancellation handle.
///
/// # IRQL
/// The future is polled at DISPATCH_LEVEL. It must use nonpaged memory, must not
/// block, and must avoid pageable kernel APIs. For PASSIVE_LEVEL work, prefer the
/// work-item executor APIs.
///
/// # Driver unload
/// Untracked tasks can outlive driver unload. Prefer
/// [`spawn_dpc_task_cancellable_tracked`] with a [`TaskTracker`] and call
/// [`TaskTracker::drain`] during unload.
pub unsafe fn spawn_dpc_task_cancellable<F>(future: F) -> Result<CancelHandle, NTSTATUS>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    let ptr = match unsafe { Task::<F>::allocate(future, core::ptr::null()) } {
        Ok(p) => p,
        Err(s) => return Err(s),
    };

    let handle = unsafe { CancelHandle::new(ptr) };
    unsafe { TaskHeader::schedule(ptr) };
    unsafe { TaskHeader::release(ptr) };

    Ok(handle)
}

/// Spawn a future onto the kcom DPC executor, tracking outstanding tasks.
///
/// # IRQL
/// The future is polled at DISPATCH_LEVEL. It must use nonpaged memory, must not
/// block, and must avoid pageable kernel APIs. For PASSIVE_LEVEL work, prefer the
/// work-item executor APIs.
///
/// # Driver unload
/// Call [`TaskTracker::drain`] after stopping submissions to ensure all tracked
/// tasks have completed before unload.
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
pub unsafe fn spawn_dpc_task_cancellable_tracked<F>(
    tracker: &TaskTracker,
    future: F,
) -> Result<CancelHandle, NTSTATUS>
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    let ptr = match unsafe { Task::<F>::allocate(future, tracker as *const TaskTracker) } {
        Ok(p) => p,
        Err(s) => return Err(s),
    };

    let handle = unsafe { CancelHandle::new(ptr) };
    unsafe { TaskHeader::schedule(ptr) };
    unsafe { TaskHeader::release(ptr) };

    Ok(handle)
}

/// Spawn a future onto the kcom DPC executor.
///
/// # IRQL
/// The future is polled at DISPATCH_LEVEL. It must use nonpaged memory, must not
/// block, and must avoid pageable kernel APIs. For PASSIVE_LEVEL work, prefer the
/// work-item executor APIs.
///
/// # Driver unload
/// This function requires a [`TaskTracker`] to ensure all tasks have completed
/// before unload.
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
pub unsafe fn spawn_dpc_task<F>(tracker: &TaskTracker, future: F) -> NTSTATUS
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    spawn_dpc_task_tracked(tracker, future)
}

/// Compatibility wrapper for spawning a tracked DPC task.
#[cfg(all(feature = "driver", feature = "async-com-kernel"))]
pub unsafe fn spawn_dpc_task_tracked<F>(tracker: &TaskTracker, future: F) -> NTSTATUS
where
    F: Future<Output = NTSTATUS> + Send + 'static,
{
    let ptr = match unsafe { Task::<F>::allocate(future, tracker as *const TaskTracker) } {
        Ok(p) => p,
        Err(s) => return s,
    };

    unsafe { TaskHeader::schedule(ptr) };
    unsafe { TaskHeader::release(ptr) };

    STATUS_SUCCESS
}

/// Spawn a future onto the kcom executor (host stub).
#[cfg(not(feature = "driver"))]
pub fn spawn_task<F>(mut future: F) -> NTSTATUS
where
    F: Future<Output = NTSTATUS> + 'static,
{
    let waker = dummy_waker();
    let mut cx = Context::from_waker(&waker);
    let mut future = unsafe { Pin::new_unchecked(&mut future) };
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(_s) => STATUS_SUCCESS,
        Poll::Pending => STATUS_SUCCESS,
    }
}
