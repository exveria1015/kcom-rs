// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

#![allow(non_camel_case_types)]

use core::ffi::c_void;

#[repr(C)]
pub struct UNICODE_STRING {
    pub Length: u16,
    pub MaximumLength: u16,
    pub Buffer: *mut u16,
}

#[cfg(target_pointer_width = "64")]
const KEVENT_SIZE: usize = 0x18;
#[cfg(target_pointer_width = "32")]
const KEVENT_SIZE: usize = 0x10;

#[cfg_attr(target_pointer_width = "64", repr(C, align(8)))]
#[cfg_attr(target_pointer_width = "32", repr(C, align(4)))]
pub struct KEVENT {
    _data: [u8; KEVENT_SIZE],
}

#[repr(i32)]
pub enum KWAIT_REASON {
    Executive = 0,
}

#[repr(i8)]
pub enum _MODE {
    KernelMode = 0,
    UserMode = 1,
}

#[repr(i32)]
pub enum EVENT_TYPE {
    NotificationEvent = 0,
    SynchronizationEvent = 1,
}

pub use EVENT_TYPE::SynchronizationEvent;

pub const APC_LEVEL: u8 = 1;

#[link(name = "ntoskrnl")]
unsafe extern "system" {
    pub fn KeGetCurrentIrql() -> u8;
    pub fn KeInitializeEvent(event: *mut KEVENT, event_type: EVENT_TYPE, state: u8);
    pub fn KeSetEvent(event: *mut KEVENT, increment: i32, wait: u8) -> i32;
    pub fn KeWaitForSingleObject(
        object: *mut c_void,
        wait_reason: KWAIT_REASON,
        wait_mode: _MODE,
        alertable: u8,
        timeout: *mut i64,
    ) -> i32;
}
