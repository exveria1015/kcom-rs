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
    /// Returns a *stable COM interface pointer* if supported.
    ///
    /// The returned pointer must reference a vtable matching `riid`.
    /// Returning `this` is only valid for the primary interface whose vtable
    /// begins at offset 0 within the object.
    /// For other interfaces, return explicit tear-offs or aggregated pointers.
    /// The `ComObject` wrapper performs `AddRef` on returned pointers.

    fn query_interface(&self, this: *mut c_void, riid: &GUID) -> Option<*mut c_void>;
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
    fn query_interface(&self, _this: *mut c_void, _riid: &GUID) -> Option<*mut c_void> {
        // FIX: Never return `self` here. The inner object T is NOT a COM pointer.
        // The wrapper handles IID_IUNKNOWN.
        None
    }
}