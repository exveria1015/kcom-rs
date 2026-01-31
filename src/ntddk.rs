// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

#![allow(non_camel_case_types)]

pub use wdk_sys::ntddk::{
    APC_LEVEL, KeGetCurrentIrql, KeInitializeEvent, KeSetEvent, KeWaitForSingleObject,
    KWAIT_REASON, _MODE, EVENT_TYPE, KEVENT, UNICODE_STRING, MmGetSystemRoutineAddress,
};
pub use wdk_sys::ntddk::EVENT_TYPE::SynchronizationEvent;

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
pub use wdk_sys::ntddk::{
    WORK_QUEUE_TYPE, PIO_WORKITEM, DEVICE_OBJECT, IoAllocateWorkItem, IoFreeWorkItem, IoQueueWorkItem,
};

#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "KMDF"))]
pub use wdk_sys::wdf::{
    WDFOBJECT, WDFDEVICE, WDFWORKITEM, PFN_WDF_WORKITEM, WDF_WORKITEM_CONFIG,
    WDF_OBJECT_ATTRIBUTES, WDF_OBJECT_CONTEXT_TYPE_INFO, WdfWorkItemCreate,
    WdfWorkItemEnqueue, WdfObjectDelete, WdfObjectGetTypedContextWorker,
};

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
