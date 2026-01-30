// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

#![no_std]

#[doc(hidden)]
pub extern crate alloc;

#[cfg(test)]
extern crate std;

pub mod iunknown;
pub mod allocator;
#[cfg(feature = "async-com")]
pub mod executor;
pub mod macros;
pub mod smart_ptr;
#[cfg(feature = "kernel-unicode")]
pub mod unicode;
#[cfg(any(feature = "async-com-kernel", feature = "kernel-unicode"))]
pub mod ntddk;
pub mod traits;
pub mod wrapper;

pub use iunknown::{
    GUID, IUnknownVtbl, IID_IUNKNOWN, NTSTATUS, STATUS_NOINTERFACE, STATUS_SUCCESS, Status,
    StatusResult,
};
pub use paste;
#[cfg(feature = "async-impl")]
pub use async_trait::async_trait as async_impl;
#[cfg(feature = "kernel-unicode")]
pub use utf16_lit;
pub use traits::{ComImpl, ComInterfaceInfo, InterfaceVtable, IUnknown, IUnknownInterface};
pub use smart_ptr::{ComInterface, ComRc};
#[cfg(feature = "kernel-unicode")]
pub use unicode::{unicode_string_as_slice, unicode_string_to_string, OwnedUnicodeString, UnicodeStringError};
pub use wrapper::ComObject;

#[macro_export]
macro_rules! impl_com_object {
    ($ty:ty, $vtable:ty) => {
        impl $ty {
            #[inline]
            pub fn new_com(inner: Self) -> *mut core::ffi::c_void {
                $crate::wrapper::ComObject::<Self, $vtable>::new(inner)
            }

            #[inline]
            pub fn new_com_in<A>(inner: Self, alloc: A) -> *mut core::ffi::c_void
            where
                A: $crate::allocator::Allocator + Send + Sync,
            {
                $crate::wrapper::ComObject::<Self, $vtable, A>::new_in(inner, alloc)
            }

            #[inline]
            pub fn try_new_com(inner: Self) -> Option<*mut core::ffi::c_void> {
                $crate::wrapper::ComObject::<Self, $vtable>::try_new(inner)
            }

            #[inline]
            pub fn try_new_com_in<A>(inner: Self, alloc: A) -> Option<*mut core::ffi::c_void>
            where
                A: $crate::allocator::Allocator + Send + Sync,
            {
                $crate::wrapper::ComObject::<Self, $vtable, A>::try_new_in(inner, alloc)
            }

            /// # Safety
            /// `outer_unknown` must point to a valid outer IUnknown vtable.
            #[inline]
            pub fn new_com_aggregated(
                inner: Self,
                outer_unknown: *mut $crate::IUnknownVtbl,
            ) -> *mut core::ffi::c_void {
                $crate::wrapper::ComObject::<Self, $vtable>::new_aggregated(inner, outer_unknown)
            }

            /// # Safety
            /// `outer_unknown` must point to a valid outer IUnknown vtable.
            #[inline]
            pub fn new_com_aggregated_in<A>(
                inner: Self,
                outer_unknown: *mut $crate::IUnknownVtbl,
                alloc: A,
            ) -> *mut core::ffi::c_void
            where
                A: $crate::allocator::Allocator + Send + Sync,
            {
                $crate::wrapper::ComObject::<Self, $vtable, A>::new_aggregated_in(
                    inner,
                    outer_unknown,
                    alloc,
                )
            }

            /// # Safety
            /// `outer_unknown` must point to a valid outer IUnknown vtable.
            #[inline]
            pub fn try_new_com_aggregated(
                inner: Self,
                outer_unknown: *mut $crate::IUnknownVtbl,
            ) -> Option<*mut core::ffi::c_void> {
                $crate::wrapper::ComObject::<Self, $vtable>::try_new_aggregated(
                    inner,
                    outer_unknown,
                )
            }

            /// # Safety
            /// `outer_unknown` must point to a valid outer IUnknown vtable.
            #[inline]
            pub fn try_new_com_aggregated_in<A>(
                inner: Self,
                outer_unknown: *mut $crate::IUnknownVtbl,
                alloc: A,
            ) -> Option<*mut core::ffi::c_void>
            where
                A: $crate::allocator::Allocator + Send + Sync,
            {
                $crate::wrapper::ComObject::<Self, $vtable, A>::try_new_aggregated_in(
                    inner,
                    outer_unknown,
                    alloc,
                )
            }
        }
    };
}
