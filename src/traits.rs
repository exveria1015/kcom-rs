// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::ffi::c_void;

use crate::iunknown::{GUID, IUnknownVtbl, IID_IUNKNOWN};
use crate::wrapper::ComObject;

pub unsafe trait InterfaceVtable: Sized + 'static {}

pub trait ComInterfaceInfo {
    type Vtable: InterfaceVtable;
    const IID: GUID;
}

pub trait ComImpl<I: InterfaceVtable>: Sized + Sync + 'static {
    const VTABLE: &'static I;

    fn query_interface(&self, riid: &GUID) -> Option<*mut c_void>;
}

pub trait IUnknown {}

impl<T: ?Sized> IUnknown for T {}

pub struct IUnknownInterface;

impl ComInterfaceInfo for IUnknownInterface {
    type Vtable = IUnknownVtbl;
    const IID: GUID = IID_IUNKNOWN;
}

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
    fn query_interface(&self, riid: &GUID) -> Option<*mut c_void> {
        if *riid == IID_IUNKNOWN {
            Some(self as *const T as *mut c_void)
        } else {
            None
        }
    }
}
