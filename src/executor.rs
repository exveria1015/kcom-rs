// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

#![allow(unexpected_cfgs)]

use alloc::boxed::Box;
use core::future::Future;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

#[cfg(all(
    feature = "async-com-kernel",
    any(driver_model__driver_type = "WDM", driver_model__driver_type = "KMDF")
))]
mod kernel {
    use super::*;

    use wdk_sys::ntddk::{
        KeInitializeEvent, KeSetEvent, KeWaitForSingleObject, KEVENT, KWAIT_REASON,
        SynchronizationEvent, _MODE,
    };

    #[cfg(debug_assertions)]
    use wdk_sys::ntddk::{KeGetCurrentIrql, DISPATCH_LEVEL, LARGE_INTEGER, STATUS_TIMEOUT};

    /// Execute a Future synchronously in kernel mode by blocking the current thread.
    pub fn block_on<F: Future>(future: F) -> F::Output {
        #[cfg(debug_assertions)]
        unsafe {
            let irql = KeGetCurrentIrql();
            if irql >= DISPATCH_LEVEL {
                panic!("CRITICAL: Attempted to block_on at DISPATCH_LEVEL!");
            }
        }

        let mut event: KEVENT = unsafe { core::mem::zeroed() };
        unsafe {
            KeInitializeEvent(&mut event, SynchronizationEvent, 0);
        }

        let waker = unsafe { Waker::from_raw(raw_waker(&mut event)) };
        let mut cx = Context::from_waker(&waker);

        let mut future = Box::pin(future);

        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(result) => return result,
                Poll::Pending => unsafe {
                    #[cfg(debug_assertions)]
                    let mut timeout = LARGE_INTEGER { QuadPart: -50_000_000 };
                    #[cfg(debug_assertions)]
                    let timeout_ptr = &mut timeout as *mut LARGE_INTEGER;
                    #[cfg(not(debug_assertions))]
                    let timeout_ptr = core::ptr::null_mut();

                    let status = KeWaitForSingleObject(
                        &mut event as *mut _ as *mut _,
                        KWAIT_REASON::Executive,
                        _MODE::KernelMode as i8,
                        0,
                        timeout_ptr as *mut _,
                    );

                    #[cfg(debug_assertions)]
                    if status == STATUS_TIMEOUT {
                        panic!("DEADLOCK DETECTED in async block!");
                    }
                },
            }
        }
    }

    unsafe fn raw_waker(ptr: *mut KEVENT) -> RawWaker {
        RawWaker::new(ptr as *const (), &VTABLE)
    }

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone_waker, wake, wake_by_ref, drop_waker);

    unsafe fn clone_waker(ptr: *const ()) -> RawWaker {
        unsafe { raw_waker(ptr as *mut KEVENT) }
    }

    unsafe fn wake(ptr: *const ()) {
        unsafe { wake_by_ref(ptr) }
    }

    unsafe fn wake_by_ref(ptr: *const ()) {
        let event = ptr as *mut KEVENT;
        unsafe {
            KeSetEvent(event, 0, 0);
        }
    }

    unsafe fn drop_waker(_ptr: *const ()) {}
}

#[cfg(not(all(
    feature = "async-com-kernel",
    any(driver_model__driver_type = "WDM", driver_model__driver_type = "KMDF")
)))]
mod host {
    use super::*;

    use core::sync::atomic::{AtomicBool, Ordering};

    pub fn block_on<F: Future>(future: F) -> F::Output {
        let ready = AtomicBool::new(false);
        let waker = unsafe { Waker::from_raw(raw_waker(&ready)) };
        let mut cx = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        loop {
            ready.store(false, Ordering::Release);
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(result) => return result,
                Poll::Pending => {
                    while !ready.swap(false, Ordering::Acquire) {
                        core::hint::spin_loop();
                    }
                }
            }
        }
    }

    unsafe fn raw_waker(flag: *const AtomicBool) -> RawWaker {
        RawWaker::new(flag as *const (), &VTABLE)
    }

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone_waker, wake, wake_by_ref, drop_waker);

    unsafe fn clone_waker(ptr: *const ()) -> RawWaker {
        unsafe { raw_waker(ptr as *const AtomicBool) }
    }

    unsafe fn wake(ptr: *const ()) {
        unsafe { wake_by_ref(ptr) }
    }

    unsafe fn wake_by_ref(ptr: *const ()) {
        let flag = unsafe { &*(ptr as *const AtomicBool) };
        flag.store(true, Ordering::Release);
    }

    unsafe fn drop_waker(_ptr: *const ()) {}
}

#[cfg(all(
    feature = "async-com-kernel",
    any(driver_model__driver_type = "WDM", driver_model__driver_type = "KMDF")
))]
pub use kernel::block_on;

#[cfg(not(all(
    feature = "async-com-kernel",
    any(driver_model__driver_type = "WDM", driver_model__driver_type = "KMDF")
)))]
pub use host::block_on;
