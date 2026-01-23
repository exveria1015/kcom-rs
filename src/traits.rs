// traits.rs

// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::ffi::c_void;
use crate::iunknown::{GUID, IUnknownVtbl, IID_IUNKNOWN};
use crate::wrapper::ComObject;

/// Trait marking a struct as a VTable layout.
/// # Safety
/// The struct must have the same memory layout as the COM interface VTable.
pub unsafe trait InterfaceVtable: Sized + 'static {}

/// Metadata associated with a COM interface (IID, VTable type).
pub trait ComInterfaceInfo {
    type Vtable: InterfaceVtable;
    const IID: GUID;
}

/// Implementation logic for a COM interface.
pub trait ComImpl<I: InterfaceVtable>: Sized + Sync + 'static {
    /// The VTable instance that delegates to `ComObject` shims.
    const VTABLE: &'static I;

    /// Checks if this object supports the interface `riid`.
    /// Returns a pointer to the *interface VTable* (not the inner object) if supported.
    ///
    /// The `ComObject` wrapper handles `IUnknown` automatically.
    /// You should implement this to support additional interfaces (e.g. via aggregation).
    fn query_interface(&self, riid: &GUID) -> Option<*mut c_void>;
}

/// Marker trait for any type that can be a COM object inner.
pub trait IUnknown {}
impl<T: ?Sized> IUnknown for T {}

pub struct IUnknownInterface;

impl ComInterfaceInfo for IUnknownInterface {
    type Vtable = IUnknownVtbl;
    const IID: GUID = IID_IUNKNOWN;
}

// Default implementation for IUnknown logic on the inner type.
impl<T> ComImpl<IUnknownVtbl> for T
where
    T: IUnknown + Sync + 'static,
{
    const VTABLE: &'static IUnknownVtbl = &IUnknownVtbl {
        QueryInterface: ComObject::<T, IUnknownVtbl>::shim_query_interface,
        AddRef: ComObject::<T, IUnknownVtbl>::shim_add_ref,
        Release: ComObject::<T, IUnknownVtbl>::shim_release,
    };

    #[inline]
    fn query_interface(&self, _riid: &GUID) -> Option<*mut c_void> {
        // FIX: Never return `self` here. The inner object T is NOT a COM pointer.
        // The wrapper handles IID_IUNKNOWN.
        None
    }
}