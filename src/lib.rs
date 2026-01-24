// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

#![no_std]

#[doc(hidden)]
pub extern crate alloc;

#[cfg(test)]
extern crate std;

pub mod iunknown;
#[cfg(feature = "async-com")]
pub mod executor;
pub mod macros;
pub mod smart_ptr;
#[cfg(feature = "kernel-unicode")]
pub mod unicode;
pub mod traits;
pub mod wrapper;

pub use iunknown::{GUID, IUnknownVtbl, IID_IUNKNOWN, NTSTATUS, STATUS_NOINTERFACE, STATUS_SUCCESS};
pub use paste;
#[cfg(feature = "async-impl")]
pub use async_trait::async_trait as async_impl;
pub use traits::{ComImpl, ComInterfaceInfo, InterfaceVtable, IUnknown, IUnknownInterface};
pub use smart_ptr::ComRc;
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
        }
    };
}
