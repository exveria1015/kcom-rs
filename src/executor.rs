// executor.rs

// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

/// Heap-allocated future using the global allocator.
pub type LocalBoxFuture<'a, T> =
    core::pin::Pin<crate::allocator::KBox<dyn Future<Output = T> + 'a, crate::allocator::GlobalAllocator>>;

/// Heap-allocated future using the kernel allocator.
#[cfg(feature = "driver")]
pub type KernelBoxFuture<'a, T> =
    core::pin::Pin<crate::allocator::KBox<dyn Future<Output = T> + Send + 'a, crate::allocator::WdkAllocator>>;

#[cfg(not(all(
    feature = "async-com-kernel",
    any(driver_model__driver_type = "WDM", driver_model__driver_type = "KMDF")
)))]
mod host {
    use super::*;
    use core::pin::Pin;

    unsafe fn host_waker_clone(data: *const ()) -> RawWaker {
        RawWaker::new(data, &HOST_WAKER_VTABLE)
    }

    unsafe fn host_waker_wake(_data: *const ()) {}

    unsafe fn host_waker_wake_by_ref(_data: *const ()) {}

    unsafe fn host_waker_drop(_data: *const ()) {}

    static HOST_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        host_waker_clone,
        host_waker_wake,
        host_waker_wake_by_ref,
        host_waker_drop,
    );

    fn host_waker() -> Waker {
        let raw = RawWaker::new(core::ptr::null(), &HOST_WAKER_VTABLE);
        unsafe { Waker::from_raw(raw) }
    }

    fn block_on_pinned<F: Future>(mut future: Pin<&mut F>) -> F::Output {
        let waker = host_waker();
        let mut cx = Context::from_waker(&waker);

        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(result) => return result,
                Poll::Pending => core::hint::spin_loop(),
            }
        }
    }

    /// Execute a Future synchronously and return the result.
    pub fn try_block_on<F: Future>(future: F) -> Result<F::Output, i32> {
        let mut future = pin!(future);
        Ok(block_on_pinned(future.as_mut()))
    }
}

#[cfg(all(
    feature = "async-com-kernel",
    any(driver_model__driver_type = "WDM", driver_model__driver_type = "KMDF")
))]
mod kernel {
    use super::*;
    use core::alloc::Layout;
    use core::ptr;
    use crate::ntddk::{
        KeGetCurrentIrql, KeInitializeEvent, KeSetEvent, KeWaitForSingleObject, KEVENT,
        KWAIT_REASON, SynchronizationEvent, APC_LEVEL, _MODE,
    };
    use crate::iunknown::STATUS_PENDING;
    #[cfg(driver_model__driver_type = "KMDF")]
    use crate::allocator::KBox;
    #[cfg(driver_model__driver_type = "KMDF")]
    use crate::allocator::WdkAllocator;
    #[cfg(driver_model__driver_type = "KMDF")]
    use crate::iunknown::NTSTATUS;
    #[cfg(driver_model__driver_type = "KMDF")]
    use crate::ntddk::{
        wdf_object_attributes_init, wdf_workitem_config_init, WDF_OBJECT_CONTEXT_TYPE_INFO,
        WDFDEVICE, WDFWORKITEM, WdfObjectDelete, WdfObjectGetTypedContextWorker, WdfWorkItemCreate,
        WdfWorkItemEnqueue,
    };
    #[cfg(driver_model__driver_type = "WDM")]
    use crate::allocator::KBox;
    #[cfg(driver_model__driver_type = "WDM")]
    use crate::allocator::WdkAllocator;
    #[cfg(driver_model__driver_type = "WDM")]
    use crate::iunknown::NTSTATUS;
    #[cfg(driver_model__driver_type = "WDM")]
    use crate::ntddk::{
        IoAllocateWorkItem, IoFreeWorkItem, IoQueueWorkItem, DEVICE_OBJECT, PIO_WORKITEM,
        WORK_QUEUE_TYPE,
    };

    struct KernelEvent(core::cell::UnsafeCell<KEVENT>);

    impl KernelEvent {
        fn init(&mut self) {
            unsafe {
                KeInitializeEvent(self.0.get(), SynchronizationEvent, 0);
            }
        }

        fn wait(&self) {
            unsafe {
                KeWaitForSingleObject(
                    self.0.get() as *mut _,
                    KWAIT_REASON::Executive,
                    _MODE::KernelMode as i8,
                    0,
                    core::ptr::null_mut(),
                );
            }
        }

        fn signal(&self) {
            unsafe {
                KeSetEvent(self.0.get(), 0, 0);
            }
        }
    }

    unsafe fn kernel_waker_clone(data: *const ()) -> RawWaker {
        RawWaker::new(data, &KERNEL_WAKER_VTABLE)
    }

    unsafe fn kernel_waker_wake(data: *const ()) {
        let event = &*(data as *const KernelEvent);
        event.signal();
    }

    unsafe fn kernel_waker_wake_by_ref(data: *const ()) {
        let event = &*(data as *const KernelEvent);
        event.signal();
    }

    unsafe fn kernel_waker_drop(_data: *const ()) {}

    static KERNEL_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        kernel_waker_clone,
        kernel_waker_wake,
        kernel_waker_wake_by_ref,
        kernel_waker_drop,
    );

    fn kernel_waker(event: &KernelEvent) -> Waker {
        let raw = RawWaker::new(event as *const _ as *const (), &KERNEL_WAKER_VTABLE);
        unsafe { Waker::from_raw(raw) }
    }

    fn irql_allows_block() -> bool {
        let irql = unsafe { KeGetCurrentIrql() };
        irql <= APC_LEVEL as u8
    }

    unsafe fn block_on_pinned_unchecked<F: Future>(mut future: Pin<&mut F>) -> F::Output {
        let mut event = KernelEvent(core::cell::UnsafeCell::new(unsafe { core::mem::zeroed() }));
        event.init();

        let waker = kernel_waker(&event);
        let mut cx = Context::from_waker(&waker);

        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(result) => return result,
                Poll::Pending => event.wait(),
            }
        }
    }

    unsafe fn try_block_on_pinned<F: Future>(future: Pin<&mut F>) -> Result<F::Output, i32> {
        if !irql_allows_block() {
            return Err(STATUS_PENDING);
        }
        Ok(unsafe { block_on_pinned_unchecked(future) })
    }

    /// Execute a Future synchronously when IRQL <= APC_LEVEL, otherwise return STATUS_PENDING.
    ///
    /// # Safety
    /// This function blocks the current thread when IRQL <= APC_LEVEL.
    /// Do NOT call this if the current thread owns resources (spinlocks, mutexes, etc.)
    /// needed by the future.
    ///
    /// If IRQL is too high, this returns `STATUS_PENDING`. Callers should treat
    /// this as an indication that the work must be offloaded (e.g. via WorkItems)
    /// and avoid assuming the future was polled.
    pub unsafe fn try_block_on<F: Future>(future: F) -> Result<F::Output, i32> {
        let mut future = pin!(future);
        unsafe { try_block_on_pinned(future.as_mut()) }
    }

    #[cfg(driver_model__driver_type = "KMDF")]
    struct WorkItemVtable {
        poll: unsafe fn(*mut core::ffi::c_void) -> NTSTATUS,
        drop: unsafe fn(*mut core::ffi::c_void, &WdkAllocator, Layout),
    }

    #[cfg(driver_model__driver_type = "KMDF")]
    struct WorkItemContext {
        data: *mut core::ffi::c_void,
        alloc: WdkAllocator,
        layout: Layout,
        vtable: WorkItemVtable,
        completion: Option<unsafe extern "system" fn(NTSTATUS)>,
    }

    #[cfg(driver_model__driver_type = "KMDF")]
    static WORKITEM_CONTEXT_TYPE_INFO: WDF_OBJECT_CONTEXT_TYPE_INFO = WDF_OBJECT_CONTEXT_TYPE_INFO {
        Size: core::mem::size_of::<WDF_OBJECT_CONTEXT_TYPE_INFO>() as u32,
        ContextName: core::ptr::null(),
        ContextSize: core::mem::size_of::<WorkItemContext>(),
        UniqueType: core::ptr::null(),
        EvtCleanupCallback: None,
    };

    #[cfg(driver_model__driver_type = "KMDF")]
    unsafe extern "system" fn work_item_callback(work_item: WDFWORKITEM) {
        let ctx_ptr = unsafe {
            WdfObjectGetTypedContextWorker(work_item, &WORKITEM_CONTEXT_TYPE_INFO)
                as *mut WorkItemContext
        };
        if ctx_ptr.is_null() {
            unsafe { WdfObjectDelete(work_item) };
            return;
        }
        let ctx = unsafe { &mut *ctx_ptr };
        let status = unsafe { (ctx.vtable.poll)(ctx.data) };
        if let Some(callback) = ctx.completion {
            unsafe { callback(status) };
        }
        unsafe { (ctx.vtable.drop)(ctx.data, &ctx.alloc, ctx.layout) };
        unsafe { WdfObjectDelete(work_item) };
    }

    #[cfg(driver_model__driver_type = "KMDF")]
    unsafe fn poll_future<F: Future<Output = NTSTATUS>>(data: *mut core::ffi::c_void) -> NTSTATUS {
        let future = unsafe { &mut *(data as *mut F) };
        let mut pinned = unsafe { Pin::new_unchecked(future) };
        match unsafe { try_block_on_pinned(pinned.as_mut()) } {
            Ok(status) => status,
            Err(status) => status,
        }
    }

    #[cfg(driver_model__driver_type = "KMDF")]
    unsafe fn drop_future<F>(data: *mut core::ffi::c_void, alloc: &WdkAllocator, layout: Layout) {
        unsafe { ptr::drop_in_place(data as *mut F) };
        unsafe { alloc.dealloc(data as *mut u8, layout) };
    }

    #[cfg(driver_model__driver_type = "KMDF")]
    pub unsafe fn spawn_work_item_kmdf<F>(
        device: WDFDEVICE,
        future: Pin<KBox<F, WdkAllocator>>,
        completion: Option<unsafe extern "system" fn(NTSTATUS)>,
    ) -> Result<(), NTSTATUS>
    where
        F: Future<Output = NTSTATUS> + Send + 'static,
    {
        let kbox = unsafe { Pin::into_inner_unchecked(future) };
        let (ptr, alloc, layout) = kbox.into_raw_parts();
        let context = WorkItemContext {
            data: ptr.as_ptr() as *mut core::ffi::c_void,
            alloc,
            layout,
            vtable: WorkItemVtable {
                poll: poll_future::<F>,
                drop: drop_future::<F>,
            },
            completion,
        };

        let config = wdf_workitem_config_init(Some(work_item_callback));
        let attributes = wdf_object_attributes_init(
            device as _,
            core::mem::size_of::<WorkItemContext>(),
            &WORKITEM_CONTEXT_TYPE_INFO,
        );
        let mut work_item = core::ptr::null_mut();
        let status = unsafe { WdfWorkItemCreate(&config, &attributes, &mut work_item) };
        if status < 0 {
            unsafe { drop_future::<F>(context.data, &context.alloc, context.layout) };
            return Err(status);
        }
        let ctx_ptr = unsafe {
            WdfObjectGetTypedContextWorker(work_item, &WORKITEM_CONTEXT_TYPE_INFO)
                as *mut WorkItemContext
        };
        if ctx_ptr.is_null() {
            unsafe { WdfObjectDelete(work_item) };
            unsafe { drop_future::<F>(context.data, &context.alloc, context.layout) };
            return Err(status);
        }
        unsafe { ptr::write(ctx_ptr, context) };
        unsafe { WdfWorkItemEnqueue(work_item) };
        Ok(())
    }

    #[cfg(driver_model__driver_type = "WDM")]
    struct WdmWorkItemContext {
        io_work_item: PIO_WORKITEM,
        data: *mut core::ffi::c_void,
        alloc: WdkAllocator,
        layout: Layout,
        poll: unsafe fn(*mut core::ffi::c_void) -> NTSTATUS,
        drop: unsafe fn(*mut core::ffi::c_void, &WdkAllocator, Layout),
        completion: Option<unsafe extern "system" fn(NTSTATUS)>,
    }

    #[cfg(driver_model__driver_type = "WDM")]
    unsafe extern "system" fn wdm_work_item_callback(
        _device: *mut DEVICE_OBJECT,
        context: *mut core::ffi::c_void,
    ) {
        let ctx = unsafe { &mut *(context as *mut WdmWorkItemContext) };
        let status = unsafe { (ctx.poll)(ctx.data) };
        if let Some(callback) = ctx.completion {
            unsafe { callback(status) };
        }
        let alloc = ctx.alloc;
        let layout = ctx.layout;
        let io_work_item = ctx.io_work_item;
        unsafe { (ctx.drop)(ctx.data, &alloc, layout) };
        unsafe { ptr::drop_in_place(ctx as *mut WdmWorkItemContext) };
        unsafe { alloc.dealloc(ctx as *mut _ as *mut u8, Layout::new::<WdmWorkItemContext>()) };
        unsafe { IoFreeWorkItem(io_work_item) };
    }

    #[cfg(driver_model__driver_type = "WDM")]
    unsafe fn wdm_poll_future<F: Future<Output = NTSTATUS>>(data: *mut core::ffi::c_void) -> NTSTATUS {
        let future = unsafe { &mut *(data as *mut F) };
        let mut pinned = unsafe { Pin::new_unchecked(future) };
        match unsafe { try_block_on_pinned(pinned.as_mut()) } {
            Ok(status) => status,
            Err(status) => status,
        }
    }

    #[cfg(driver_model__driver_type = "WDM")]
    unsafe fn wdm_drop_future<F>(data: *mut core::ffi::c_void, alloc: &WdkAllocator, layout: Layout) {
        unsafe { ptr::drop_in_place(data as *mut F) };
        unsafe { alloc.dealloc(data as *mut u8, layout) };
    }

    #[cfg(driver_model__driver_type = "WDM")]
    pub unsafe fn spawn_work_item_wdm<F>(
        device: *mut DEVICE_OBJECT,
        queue: WORK_QUEUE_TYPE,
        future: Pin<KBox<F, WdkAllocator>>,
        completion: Option<unsafe extern "system" fn(NTSTATUS)>,
    ) -> Result<(), NTSTATUS>
    where
        F: Future<Output = NTSTATUS> + Send + 'static,
    {
        let io_work_item = unsafe { IoAllocateWorkItem(device) };
        if io_work_item.is_null() {
            return Err(crate::iunknown::STATUS_INSUFFICIENT_RESOURCES);
        }
        let kbox = unsafe { Pin::into_inner_unchecked(future) };
        let (ptr, alloc, layout) = kbox.into_raw_parts();
        let context_layout = Layout::new::<WdmWorkItemContext>();
        let ctx_ptr = unsafe { alloc.alloc(context_layout) } as *mut WdmWorkItemContext;
        let ctx_ptr = match core::ptr::NonNull::new(ctx_ptr) {
            Some(ptr) => ptr.as_ptr(),
            None => {
                unsafe { IoFreeWorkItem(io_work_item) };
                unsafe { wdm_drop_future::<F>(ptr.as_ptr() as *mut _, &alloc, layout) };
                return Err(crate::iunknown::STATUS_INSUFFICIENT_RESOURCES);
            }
        };

        let context = WdmWorkItemContext {
            io_work_item,
            data: ptr.as_ptr() as *mut core::ffi::c_void,
            alloc,
            layout,
            poll: wdm_poll_future::<F>,
            drop: wdm_drop_future::<F>,
            completion,
        };

        unsafe { ptr::write(ctx_ptr, context) };
        unsafe {
            IoQueueWorkItem(
                io_work_item,
                wdm_work_item_callback,
                queue,
                ctx_ptr as *mut core::ffi::c_void,
            );
        }
        Ok(())
    }
}

#[cfg(all(
    feature = "async-com-kernel",
    any(driver_model__driver_type = "WDM", driver_model__driver_type = "KMDF")
))]
pub use kernel::try_block_on;

#[cfg(driver_model__driver_type = "KMDF")]
pub use kernel::spawn_work_item_kmdf;
#[cfg(driver_model__driver_type = "WDM")]
pub use kernel::spawn_work_item_wdm;

#[cfg(not(all(
    feature = "async-com-kernel",
    any(driver_model__driver_type = "WDM", driver_model__driver_type = "KMDF")
)))]
pub use host::try_block_on;