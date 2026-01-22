// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::ffi::c_void;

use crate::traits::InterfaceVtable;

pub type NTSTATUS = i32;

pub const STATUS_SUCCESS: NTSTATUS = 0;
pub const STATUS_NOINTERFACE: NTSTATUS = 0xC000_02B9u32 as i32;

pub trait IntoNtStatus {
    fn into_ntstatus(self) -> NTSTATUS;
}

impl IntoNtStatus for NTSTATUS {
    #[inline]
    fn into_ntstatus(self) -> NTSTATUS {
        self
    }
}

impl<T, E> IntoNtStatus for Result<T, E>
where
    E: Into<NTSTATUS>,
{
    #[inline]
    fn into_ntstatus(self) -> NTSTATUS {
        match self {
            Ok(_) => STATUS_SUCCESS,
            Err(err) => err.into(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const IID_IUNKNOWN: GUID = GUID {
    data1: 0x0000_0000,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_snake_case)]
pub struct IUnknownVtbl {
    pub QueryInterface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> NTSTATUS,
    pub AddRef: unsafe extern "system" fn(*mut c_void) -> u32,
    pub Release: unsafe extern "system" fn(*mut c_void) -> u32,
}

unsafe impl InterfaceVtable for IUnknownVtbl {}
