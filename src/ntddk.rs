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

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
#[repr(i32)]
pub enum WORK_QUEUE_TYPE {
    CriticalWorkQueue = 0,
    DelayedWorkQueue = 1,
    HyperCriticalWorkQueue = 2,
}

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
pub type PIO_WORKITEM = *mut c_void;

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
#[repr(C)]
pub struct DEVICE_OBJECT {
    _padding: [u8; 0],
}

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
#[link(name = "ntoskrnl")]
unsafe extern "system" {
    pub fn IoAllocateWorkItem(device_object: *mut DEVICE_OBJECT) -> PIO_WORKITEM;
    pub fn IoFreeWorkItem(io_work_item: PIO_WORKITEM);
    pub fn IoQueueWorkItem(
        io_work_item: PIO_WORKITEM,
        routine: unsafe extern "system" fn(*mut DEVICE_OBJECT, *mut c_void),
        queue_type: WORK_QUEUE_TYPE,
        context: *mut c_void,
    );
}

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "KMDF"))]
pub type WDFOBJECT = *mut c_void;
#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "KMDF"))]
pub type WDFDEVICE = WDFOBJECT;
#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "KMDF"))]
pub type WDFWORKITEM = WDFOBJECT;

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "KMDF"))]
pub type PFN_WDF_WORKITEM = Option<unsafe extern "system" fn(WDFWORKITEM)>;

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "KMDF"))]
#[repr(C)]
pub struct WDF_WORKITEM_CONFIG {
    pub Size: u32,
    pub EvtWorkItem: PFN_WDF_WORKITEM,
    pub AutomaticSerialization: u8,
    pub _padding: [u8; 3],
}

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "KMDF"))]
#[repr(C)]
pub struct WDF_OBJECT_ATTRIBUTES {
    pub Size: u32,
    pub EvtCleanupCallback: Option<unsafe extern "system" fn(WDFOBJECT)>,
    pub EvtDestroyCallback: Option<unsafe extern "system" fn(WDFOBJECT)>,
    pub ExecutionLevel: u32,
    pub SynchronizationScope: u32,
    pub ParentObject: WDFOBJECT,
    pub ContextSizeOverride: usize,
    pub ContextTypeInfo: *const WDF_OBJECT_CONTEXT_TYPE_INFO,
}

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "KMDF"))]
#[repr(C)]
pub struct WDF_OBJECT_CONTEXT_TYPE_INFO {
    pub Size: u32,
    pub ContextName: *const u8,
    pub ContextSize: usize,
    pub UniqueType: *const WDF_OBJECT_CONTEXT_TYPE_INFO,
    pub EvtCleanupCallback: Option<unsafe extern "system" fn(WDFOBJECT)>,
}

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "KMDF"))]
#[inline]
pub fn wdf_workitem_config_init(evt: PFN_WDF_WORKITEM) -> WDF_WORKITEM_CONFIG {
    WDF_WORKITEM_CONFIG {
        Size: core::mem::size_of::<WDF_WORKITEM_CONFIG>() as u32,
        EvtWorkItem: evt,
        AutomaticSerialization: 0,
        _padding: [0; 3],
    }
}

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "KMDF"))]
#[inline]
pub fn wdf_object_attributes_init(
    parent: WDFOBJECT,
    context_size: usize,
    context_type_info: *const WDF_OBJECT_CONTEXT_TYPE_INFO,
) -> WDF_OBJECT_ATTRIBUTES {
    WDF_OBJECT_ATTRIBUTES {
        Size: core::mem::size_of::<WDF_OBJECT_ATTRIBUTES>() as u32,
        EvtCleanupCallback: None,
        EvtDestroyCallback: None,
        ExecutionLevel: 0,
        SynchronizationScope: 0,
        ParentObject: parent,
        ContextSizeOverride: context_size,
        ContextTypeInfo: context_type_info,
    }
}

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "KMDF"))]
#[link(name = "WdfDriverEntry")]
unsafe extern "system" {
    pub fn WdfWorkItemCreate(
        config: *const WDF_WORKITEM_CONFIG,
        attributes: *const WDF_OBJECT_ATTRIBUTES,
        work_item: *mut WDFWORKITEM,
    ) -> i32;
    pub fn WdfWorkItemEnqueue(work_item: WDFWORKITEM);
    pub fn WdfObjectDelete(object: WDFOBJECT);
    pub fn WdfObjectGetTypedContextWorker(
        object: WDFOBJECT,
        type_info: *const WDF_OBJECT_CONTEXT_TYPE_INFO,
    ) -> *mut c_void;
}
