// executor.rs

// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::task::Wake;
use core::future::Future;
use core::task::{Context, Poll, Waker};

#[cfg(all(
    feature = "async-com-kernel",
    any(driver_model__driver_type = "WDM", driver_model__driver_type = "KMDF")
))]
mod kernel {
    use super::*;
    use wdk_sys::ntddk::{
        KeGetCurrentIrql, KeInitializeEvent, KeSetEvent, KeWaitForSingleObject, KEVENT,
        KWAIT_REASON, SynchronizationEvent, APC_LEVEL, _MODE,
    };

    // SAFETY: KEVENT is thread-safe for synchronization.
    struct KernelEvent(core::cell::UnsafeCell<KEVENT>);
    unsafe impl Send for KernelEvent {}
    unsafe impl Sync for KernelEvent {}

    impl KernelEvent {
        fn new() -> Arc<Self> {
            let event = Arc::new(Self(core::cell::UnsafeCell::new(unsafe { core::mem::zeroed() })));
            unsafe {
                KeInitializeEvent(event.0.get(), SynchronizationEvent, 0);
            }
            event
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

    impl Wake for KernelEvent {
        fn wake(self: Arc<Self>) {
            self.signal();
        }
        
        fn wake_by_ref(self: &Arc<Self>) {
            self.signal();
        }
    }

    fn check_irql() {
        let irql = unsafe { KeGetCurrentIrql() };
        debug_assert!(
            irql <= APC_LEVEL as u8,
            "block_on requires IRQL <= APC_LEVEL"
        );
    }

    /// Execute a Future synchronously in kernel mode.
    ///
    /// # Safety
    /// This function blocks the current thread.
    /// - Must be called at IRQL <= APC_LEVEL.
    /// - Do NOT call this if the current thread owns resources that might cause a deadlock.
    pub fn block_on<F: Future>(future: F) -> F::Output {
        check_irql();

        let event = KernelEvent::new();
        let waker = Waker::from(event.clone());
        let mut cx = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(result) => return result,
                Poll::Pending => event.wait(),
            }
        }
    }
}

#[cfg(not(all(
    feature = "async-com-kernel",
    any(driver_model__driver_type = "WDM", driver_model__driver_type = "KMDF")
)))]
mod host {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering};
    
    // Polyfill or import WaitOnAddress from windows-sys / winapi
    // Here we assume windows-sys is available or we link against Synchronization.lib
    #[link(name = "Synchronization")]
    unsafe extern "system" {
        fn WaitOnAddress(
            Address: *const core::ffi::c_void,
            CompareAddress: *const core::ffi::c_void,
            AddressSize: usize,
            dwMilliseconds: u32,
        ) -> i32;
        
        fn WakeByAddressSingle(Address: *const core::ffi::c_void);
    }

    const PENDING: u32 = 0;
    const NOTIFIED: u32 = 1;

    struct HostSignal(AtomicU32);

    impl Wake for HostSignal {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            if self.0.swap(NOTIFIED, Ordering::Release) == PENDING {
                unsafe {
                    WakeByAddressSingle(self.0.as_ptr() as *const _);
                }
            }
        }
    }

    pub fn block_on<F: Future>(future: F) -> F::Output {
        let signal = Arc::new(HostSignal(AtomicU32::new(PENDING)));
        let waker = Waker::from(signal.clone());
        let mut cx = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(result) => return result,
                Poll::Pending => {
                    let mut value = signal.0.load(Ordering::Acquire);
                    while value == PENDING {
                         unsafe {
                             WaitOnAddress(
                                 signal.0.as_ptr() as *const _,
                                 &PENDING as *const _ as *const _,
                                 4,
                                 0xFFFFFFFF // INFINITE
                             );
                         }
                         value = signal.0.load(Ordering::Acquire);
                    }
                    // Reset for next poll
                    signal.0.store(PENDING, Ordering::Release);
                }
            }
        }
    }
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