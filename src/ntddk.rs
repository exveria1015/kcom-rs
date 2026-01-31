// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

#![allow(non_camel_case_types)]

pub use wdk_sys::{
    APC_LEVEL, DISPATCH_LEVEL, EVENT_TYPE, KWAIT_REASON, KEVENT, UNICODE_STRING, _EVENT_TYPE,
    _KWAIT_REASON, _MODE,
};
pub use wdk_sys::{KDPC, KTIMER, LARGE_INTEGER, PKDPC, PKTIMER};
pub use wdk_sys::{KIRQL, KSPIN_LOCK};
pub use wdk_sys::ntddk::{
    KeAcquireSpinLockRaiseToDpc, KeCancelTimer, KeGetCurrentIrql, KeInitializeDpc,
    KeInitializeEvent, KeInitializeSpinLock, KeInitializeTimer, KeInsertQueueDpc,
    KeReleaseSpinLock, KeRemoveQueueDpc, KeSetEvent, KeSetTimer, KeWaitForSingleObject,
    MmGetSystemRoutineAddress,
};
pub use wdk_sys::_EVENT_TYPE::SynchronizationEvent;


#[cfg(all(feature = "async-com-kernel", driver_model__driver_type = "WDM"))]
pub use wdk_sys::ntddk::{
    DEVICE_OBJECT, IoAllocateWorkItem, IoFreeWorkItem, IoQueueWorkItem, ObDereferenceObject,
    ObReferenceObject, PIO_WORKITEM, WORK_QUEUE_TYPE,
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
