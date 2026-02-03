// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

#![no_std]

#[doc(hidden)]
pub extern crate alloc;

#[cfg(test)]
extern crate std;

pub mod iunknown;
pub mod allocator;
#[cfg(all(feature = "driver", feature = "driver-test-stub"))]
mod driver_test_stub;
pub mod executor;
pub mod macros;
pub use macros::*;
pub mod smart_ptr;
pub mod task;
pub mod vtable;
mod refcount;
pub mod trace;
mod guard_ptr;
#[cfg(feature = "async-com")]
pub mod async_com;
#[cfg(feature = "kernel-unicode")]
pub mod unicode;
#[cfg(any(feature = "async-com-kernel", feature = "kernel-unicode"))]
pub mod ntddk;
pub mod traits;
pub mod wrapper;

pub use iunknown::{
    GUID, IUnknownVtbl, IID_IUNKNOWN, NTSTATUS, PendingResult, STATUS_INVALID_PARAMETER,
    STATUS_NOINTERFACE, STATUS_SUCCESS, Status, StatusResult,
};
pub use paste;
#[cfg(feature = "async-impl")]
pub use async_trait::async_trait as async_impl;
#[cfg(feature = "kernel-unicode")]
pub use utf16_lit;
pub use traits::{ComImpl, IUnknown, IUnknownInterface};
pub use vtable::{ComInterfaceInfo, InterfaceVtable, match_interface_ptr};
pub use smart_ptr::{ComInterface, ComRc, ThreadSafeComInterface};
pub use trace::{clear_trace_hook, set_trace_hook, TraceHook};
pub use allocator::{
    Allocator, GlobalAllocator, InitBox, InitBoxTrait, KBox, KBoxError, PinInit, PinInitOnce,
};
#[cfg(feature = "driver")]
pub use allocator::{init_box_with_tag, KernelInitBox, PoolType, WdkAllocator};
#[cfg(all(feature = "driver", not(miri)))]
pub use allocator::init_ex_allocate_pool2;
#[cfg(feature = "kernel-unicode")]
pub use unicode::{
    unicode_string_as_slice,
    unicode_string_to_string,
    LocalUnicodeString,
    OwnedUnicodeString,
    UnicodeStringError,
};
pub use wrapper::{ComObject, ComObjectN};
#[doc(hidden)]
pub use guard_ptr::GuardPtr;

#[cfg(feature = "async-com")]
pub use async_com::{
    spawn_async_operation,
    spawn_async_operation_cancellable,
    spawn_async_operation_error,
    spawn_async_operation_raw,
    spawn_async_operation_raw_cancellable,
    spawn_async_operation_error_raw,
    AsyncOperationRaw,
    AsyncOperationTask,
    AsyncOperationVtbl,
    AsyncStatus,
    AsyncValueType,
};

pub use executor::{spawn_dpc_task_cancellable, CancelHandle};
#[cfg(any(
    not(feature = "driver"),
    miri,
    all(feature = "driver", feature = "async-com-kernel", not(miri))
))]
pub use executor::spawn_task;
#[cfg(all(feature = "driver", feature = "async-com-kernel", not(miri)))]
pub use executor::{
    set_task_alloc_tag,
    set_task_budget,
    spawn_dpc_task,
    spawn_dpc_task_cancellable_tracked,
    spawn_dpc_task_tracked,
    TaskBudget,
    TaskTracker,
};
pub use task::{try_finally, Cancellable};
#[cfg(all(feature = "driver", feature = "async-com-kernel", not(miri)))]
pub use executor::KernelTimerFuture;
#[cfg(all(feature = "driver", feature = "async-com-kernel", not(miri)))]
pub use executor::{
    spawn_task_cancellable,
    DefaultTaskContext,
    TaskContext,
    TaskContextCallback,
    WorkItemCancelHandle,
};
#[cfg(all(
    feature = "driver",
    feature = "async-com-kernel",
    driver_model__driver_type = "WDM",
    not(miri)
))]
pub use executor::{
    spawn_task_cancellable_tracked,
    spawn_task_tracked,
    WorkItemTracker,
};

#[macro_export]
macro_rules! impl_com_object {
    ($ty:ty, $vtable:ty) => {
        #[allow(dead_code)]
        impl $ty {
            #[inline]
            pub fn new_com(inner: Self) -> Result<*mut core::ffi::c_void, $crate::NTSTATUS> {
                $crate::wrapper::ComObject::<Self, $vtable>::new(inner)
            }

            #[inline]
            pub fn new_com_rc<R>(
                inner: Self,
            ) -> Result<$crate::smart_ptr::ComRc<R>, $crate::NTSTATUS>
            where
                R: $crate::smart_ptr::ComInterface
                    + $crate::vtable::ComInterfaceInfo<Vtable = $vtable>,
            {
                $crate::wrapper::ComObject::<Self, $vtable>::new_rc(inner)
            }

            #[inline]
            pub fn new_com_in<A>(
                inner: Self,
                alloc: A,
            ) -> Result<*mut core::ffi::c_void, $crate::NTSTATUS>
            where
                A: $crate::allocator::Allocator + Send + Sync,
            {
                $crate::wrapper::ComObject::<Self, $vtable, A>::new_in(inner, alloc)
            }

            #[inline]
            pub fn new_com_rc_in<A, R>(
                inner: Self,
                alloc: A,
            ) -> Result<$crate::smart_ptr::ComRc<R>, $crate::NTSTATUS>
            where
                A: $crate::allocator::Allocator + Send + Sync,
                R: $crate::smart_ptr::ComInterface
                    + $crate::vtable::ComInterfaceInfo<Vtable = $vtable>,
            {
                $crate::wrapper::ComObject::<Self, $vtable, A>::new_rc_in(inner, alloc)
            }

            #[inline]
            pub fn try_new_com(inner: Self) -> Option<*mut core::ffi::c_void> {
                $crate::wrapper::ComObject::<Self, $vtable>::try_new(inner)
            }

            #[inline]
            pub fn try_new_com_rc<R>(inner: Self) -> Option<$crate::smart_ptr::ComRc<R>>
            where
                R: $crate::smart_ptr::ComInterface
                    + $crate::vtable::ComInterfaceInfo<Vtable = $vtable>,
            {
                $crate::wrapper::ComObject::<Self, $vtable>::try_new_rc(inner)
            }

            #[inline]
            pub fn try_new_com_in<A>(inner: Self, alloc: A) -> Option<*mut core::ffi::c_void>
            where
                A: $crate::allocator::Allocator + Send + Sync,
            {
                $crate::wrapper::ComObject::<Self, $vtable, A>::try_new_in(inner, alloc)
            }

            #[inline]
            pub fn try_new_com_rc_in<A, R>(
                inner: Self,
                alloc: A,
            ) -> Option<$crate::smart_ptr::ComRc<R>>
            where
                A: $crate::allocator::Allocator + Send + Sync,
                R: $crate::smart_ptr::ComInterface
                    + $crate::vtable::ComInterfaceInfo<Vtable = $vtable>,
            {
                $crate::wrapper::ComObject::<Self, $vtable, A>::try_new_rc_in(inner, alloc)
            }

            /// # Safety
            /// `outer_unknown` must point to a valid outer IUnknown interface pointer.
            #[inline]
            pub unsafe fn new_com_aggregated(
                inner: Self,
                outer_unknown: *mut core::ffi::c_void,
            ) -> Result<*mut core::ffi::c_void, $crate::NTSTATUS> {
                unsafe {
                    $crate::wrapper::ComObject::<Self, $vtable>::new_aggregated(inner, outer_unknown)
                }
            }

            /// # Safety
            /// `outer_unknown` must point to a valid outer IUnknown interface pointer.
            #[inline]
            pub unsafe fn new_com_aggregated_in<A>(
                inner: Self,
                outer_unknown: *mut core::ffi::c_void,
                alloc: A,
            ) -> Result<*mut core::ffi::c_void, $crate::NTSTATUS>
            where
                A: $crate::allocator::Allocator + Send + Sync,
            {
                unsafe {
                    $crate::wrapper::ComObject::<Self, $vtable, A>::new_aggregated_in(
                        inner,
                        outer_unknown,
                        alloc,
                    )
                }
            }

            /// # Safety
            /// `outer_unknown` must point to a valid outer IUnknown interface pointer.
            #[inline]
            pub unsafe fn try_new_com_aggregated(
                inner: Self,
                outer_unknown: *mut core::ffi::c_void,
            ) -> Option<*mut core::ffi::c_void> {
                unsafe {
                    $crate::wrapper::ComObject::<Self, $vtable>::try_new_aggregated(
                        inner,
                        outer_unknown,
                    )
                }
            }

            /// # Safety
            /// `outer_unknown` must point to a valid outer IUnknown interface pointer.
            #[inline]
            pub unsafe fn try_new_com_aggregated_in<A>(
                inner: Self,
                outer_unknown: *mut core::ffi::c_void,
                alloc: A,
            ) -> Option<*mut core::ffi::c_void>
            where
                A: $crate::allocator::Allocator + Send + Sync,
            {
                unsafe {
                    $crate::wrapper::ComObject::<Self, $vtable, A>::try_new_aggregated_in(
                        inner,
                        outer_unknown,
                        alloc,
                    )
                }
            }
        }
    };
}
