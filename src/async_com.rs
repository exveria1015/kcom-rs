// async_com.rs
//
// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::future::Future;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};
#[cfg(test)]
use core::sync::atomic::AtomicUsize;

use crate::executor::{spawn_cancellable_task, CancelHandle};
use crate::iunknown::{
    GUID, IUnknownVtbl, NTSTATUS, STATUS_CANCELLED, STATUS_PENDING, STATUS_SUCCESS,
    STATUS_UNSUCCESSFUL,
};
use crate::smart_ptr::{ComInterface, ComRc};
use crate::traits::ComImpl;
use crate::vtable::InterfaceVtable;
use crate::wrapper::ComObject;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsyncStatus {
    Started = 0,
    Completed = 1,
    Canceled = 2,
    Error = 3,
}

impl AsyncStatus {
    #[inline]
    pub const fn as_raw(self) -> u32 {
        self as u32
    }

    #[inline]
    pub const fn from_raw(value: u32) -> Self {
        match value {
            0 => Self::Started,
            1 => Self::Completed,
            2 => Self::Canceled,
            _ => Self::Error,
        }
    }
}

pub trait AsyncValueType: Copy + Send + Sync + 'static {}

impl<T> AsyncValueType for T where T: Copy + Send + Sync + 'static {}

#[repr(C)]
pub struct AsyncOperationVtbl<T: AsyncValueType> {
    pub parent: IUnknownVtbl,
    pub get_status: unsafe extern "system" fn(*mut c_void, *mut AsyncStatus) -> NTSTATUS,
    pub get_result: unsafe extern "system" fn(*mut c_void, *mut T) -> NTSTATUS,
}

unsafe impl<T: AsyncValueType> InterfaceVtable for AsyncOperationVtbl<T> {}

impl<T: AsyncValueType> AsyncOperationVtbl<T> {
    pub const fn new<F>() -> Self
    where
        F: Future<Output = T> + Send + 'static,
    {
        Self {
            parent: IUnknownVtbl::new::<AsyncOperationTask<T, F>, Self>(),
            get_status: AsyncOperationTask::<T, F>::shim_get_status,
            get_result: AsyncOperationTask::<T, F>::shim_get_result,
        }
    }
}

#[repr(C)]
#[allow(non_snake_case)]
pub struct AsyncOperationRaw<T: AsyncValueType> {
    pub lpVtbl: *mut AsyncOperationVtbl<T>,
}

unsafe impl<T: AsyncValueType> ComInterface for AsyncOperationRaw<T> {}

impl<T: AsyncValueType> AsyncOperationRaw<T> {
    #[inline]
    pub unsafe fn get_status(&self) -> Result<AsyncStatus, NTSTATUS> {
        let vtbl = self.lpVtbl;
        if vtbl.is_null() {
            return Err(STATUS_UNSUCCESSFUL);
        }
        let mut status = AsyncStatus::Started;
        let result = unsafe {
            ((*vtbl).get_status)(
                self as *const _ as *mut c_void,
                &mut status as *mut AsyncStatus,
            )
        };
        if result < 0 {
            Err(result)
        } else {
            Ok(status)
        }
    }

    #[inline]
    pub unsafe fn get_result(&self) -> Result<T, NTSTATUS> {
        let vtbl = self.lpVtbl;
        if vtbl.is_null() {
            return Err(STATUS_UNSUCCESSFUL);
        }
        let mut out = core::mem::MaybeUninit::<T>::uninit();
        let result = unsafe {
            ((*vtbl).get_result)(
                self as *const _ as *mut c_void,
                out.as_mut_ptr(),
            )
        };
        if result == STATUS_SUCCESS {
            Ok(unsafe { out.assume_init() })
        } else {
            Err(result)
        }
    }
}

pub struct AsyncOperationTask<T, F>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    status: AtomicU32,
    error: AtomicI32,
    result: UnsafeCell<MaybeUninit<T>>,
    _marker: PhantomData<F>,
}

#[cfg(test)]
static ASYNC_OPERATION_DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl<T, F> Sync for AsyncOperationTask<T, F>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
}

#[cfg(test)]
impl<T, F> Drop for AsyncOperationTask<T, F>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    fn drop(&mut self) {
        ASYNC_OPERATION_DROP_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

impl<T, F> AsyncOperationTask<T, F>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    #[inline]
    fn new_state() -> Self {
        Self {
            status: AtomicU32::new(AsyncStatus::Started.as_raw()),
            error: AtomicI32::new(STATUS_UNSUCCESSFUL),
            result: UnsafeCell::new(MaybeUninit::uninit()),
            _marker: PhantomData,
        }
    }

    #[inline]
    fn store_result(&self, value: T) {
        unsafe {
            (*self.result.get()).write(value);
        }
        self.error.store(STATUS_SUCCESS, Ordering::Release);
        self.status
            .store(AsyncStatus::Completed.as_raw(), Ordering::Release);
    }

    #[inline]
    fn store_error(&self, status: NTSTATUS) {
        self.error.store(status, Ordering::Release);
        self.status
            .store(AsyncStatus::Error.as_raw(), Ordering::Release);
    }

    #[inline]
    fn store_canceled(&self) {
        self.error.store(STATUS_CANCELLED, Ordering::Release);
        self.status
            .store(AsyncStatus::Canceled.as_raw(), Ordering::Release);
    }

    #[inline]
    fn load_status(&self) -> AsyncStatus {
        AsyncStatus::from_raw(self.status.load(Ordering::Acquire))
    }

    pub fn spawn_raw(future: F) -> Result<*mut AsyncOperationRaw<T>, NTSTATUS> {
        let (ptr, _handle) = Self::spawn_raw_cancellable(future)?;
        Ok(ptr)
    }

    pub fn spawn_raw_cancellable(
        future: F,
    ) -> Result<(*mut AsyncOperationRaw<T>, CancelHandle), NTSTATUS> {
        let ptr = ComObject::<Self, AsyncOperationVtbl<T>>::new(Self::new_state())?;

        // Hold a reference while the async task runs.
        unsafe {
            ComObject::<Self, AsyncOperationVtbl<T>>::shim_add_ref(ptr);
        }

        struct TaskGuard<T, F>
        where
            T: AsyncValueType,
            F: Future<Output = T> + Send + 'static,
        {
            ptr: usize,
            _marker: PhantomData<(T, F)>,
        }

        impl<T, F> Drop for TaskGuard<T, F>
        where
            T: AsyncValueType,
            F: Future<Output = T> + Send + 'static,
        {
            fn drop(&mut self) {
                let ptr = self.ptr as *mut c_void;
                unsafe {
                    ComObject::<AsyncOperationTask<T, F>, AsyncOperationVtbl<T>>::shim_release(
                        ptr,
                    );
                }
            }
        }

        let task_ptr = ptr as usize;
        let task = async move {
            let _guard = TaskGuard::<T, F> {
                ptr: task_ptr,
                _marker: PhantomData,
            };

            let result = crate::task::try_finally(future, async {}).await;
            let task_ptr = task_ptr as *mut c_void;
            let wrapper =
                unsafe { ComObject::<Self, AsyncOperationVtbl<T>>::from_ptr(task_ptr) };
            match result {
                Some(value) => wrapper.inner.store_result(value),
                None => wrapper.inner.store_canceled(),
            }
            STATUS_SUCCESS
        };

        let handle = match spawn_cancellable_task(task) {
            Ok(handle) => handle,
            Err(status) => {
                unsafe {
                    ComObject::<Self, AsyncOperationVtbl<T>>::shim_release(ptr);
                    ComObject::<Self, AsyncOperationVtbl<T>>::shim_release(ptr);
                }
                return Err(status);
            }
        };

        Ok((ptr as *mut AsyncOperationRaw<T>, handle))
    }

    pub fn spawn_error_raw(status: NTSTATUS) -> Result<*mut AsyncOperationRaw<T>, NTSTATUS> {
        let task = Self::new_state();
        task.store_error(status);
        let ptr = ComObject::<Self, AsyncOperationVtbl<T>>::new(task)?;
        Ok(ptr as *mut AsyncOperationRaw<T>)
    }

    #[inline]
    pub fn spawn(future: F) -> Result<ComRc<AsyncOperationRaw<T>>, NTSTATUS> {
        let ptr = Self::spawn_raw(future)?;
        Ok(unsafe { ComRc::from_raw_unchecked(ptr) })
    }

    #[inline]
    pub fn spawn_cancellable(
        future: F,
    ) -> Result<(ComRc<AsyncOperationRaw<T>>, CancelHandle), NTSTATUS> {
        let (ptr, handle) = Self::spawn_raw_cancellable(future)?;
        let op = unsafe { ComRc::from_raw_unchecked(ptr) };
        Ok((op, handle))
    }

    #[allow(non_snake_case)]
    pub unsafe extern "system" fn shim_get_status(
        this: *mut c_void,
        out_status: *mut AsyncStatus,
    ) -> NTSTATUS {
        if this.is_null() || out_status.is_null() {
            return STATUS_UNSUCCESSFUL;
        }
        let wrapper = unsafe { ComObject::<Self, AsyncOperationVtbl<T>>::from_ptr(this) };
        let status = wrapper.inner.load_status();
        unsafe {
            *out_status = status;
        }
        STATUS_SUCCESS
    }

    #[allow(non_snake_case)]
    pub unsafe extern "system" fn shim_get_result(
        this: *mut c_void,
        out_result: *mut T,
    ) -> NTSTATUS {
        if this.is_null() || out_result.is_null() {
            return STATUS_UNSUCCESSFUL;
        }
        let wrapper = unsafe { ComObject::<Self, AsyncOperationVtbl<T>>::from_ptr(this) };
        match wrapper.inner.load_status() {
            AsyncStatus::Completed => {
                let value = unsafe { (*wrapper.inner.result.get()).assume_init() };
                unsafe {
                    out_result.write(value);
                }
                STATUS_SUCCESS
            }
            AsyncStatus::Started => STATUS_PENDING,
            AsyncStatus::Canceled | AsyncStatus::Error => wrapper.inner.error.load(Ordering::Acquire),
        }
    }
}

impl<T, F> ComImpl<AsyncOperationVtbl<T>> for AsyncOperationTask<T, F>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    const VTABLE: &'static AsyncOperationVtbl<T> = &AsyncOperationVtbl::<T>::new::<F>();

    #[inline]
    fn query_interface(&self, _this: *mut c_void, _riid: &GUID) -> Option<*mut c_void> {
        None
    }
}

#[inline]
pub fn spawn_async_operation_raw<T, F>(future: F) -> Result<*mut AsyncOperationRaw<T>, NTSTATUS>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    AsyncOperationTask::<T, F>::spawn_raw(future)
}

#[inline]
pub fn spawn_async_operation_raw_cancellable<T, F>(
    future: F,
) -> Result<(*mut AsyncOperationRaw<T>, CancelHandle), NTSTATUS>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    AsyncOperationTask::<T, F>::spawn_raw_cancellable(future)
}

#[inline]
pub fn spawn_async_operation_error_raw<T, F>(status: NTSTATUS) -> Result<*mut AsyncOperationRaw<T>, NTSTATUS>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    AsyncOperationTask::<T, F>::spawn_error_raw(status)
}

#[inline]
pub fn spawn_async_operation<T, F>(future: F) -> Result<ComRc<AsyncOperationRaw<T>>, NTSTATUS>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    AsyncOperationTask::<T, F>::spawn(future)
}

#[inline]
pub fn spawn_async_operation_cancellable<T, F>(
    future: F,
) -> Result<(ComRc<AsyncOperationRaw<T>>, CancelHandle), NTSTATUS>
where
    T: AsyncValueType,
    F: Future<Output = T> + Send + 'static,
{
    AsyncOperationTask::<T, F>::spawn_cancellable(future)
}

#[inline]
pub fn spawn_async_operation_error<T>(status: NTSTATUS) -> Result<ComRc<AsyncOperationRaw<T>>, NTSTATUS>
where
    T: AsyncValueType,
{
    let ptr = spawn_async_operation_error_raw::<T, core::future::Ready<T>>(status)?;
    Ok(unsafe { ComRc::from_raw_unchecked(ptr) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(not(feature = "driver"))]
    use core::sync::atomic::Ordering;

    #[test]
    fn ready_future_completes() {
        let _guard = TEST_LOCK.lock().unwrap();
        let op = spawn_async_operation(async { 11u32 }).expect("spawn async operation");
        unsafe {
            let status = op.get_status().expect("get status");
            assert_eq!(status, AsyncStatus::Completed);
            let result = op.get_result().expect("get result");
            assert_eq!(result, 11u32);
        }
    }

    #[test]
    fn error_operation_reports_error() {
        let _guard = TEST_LOCK.lock().unwrap();
        let op = spawn_async_operation_error::<u32>(STATUS_UNSUCCESSFUL).expect("spawn error op");
        unsafe {
            let status = op.get_status().expect("get status");
            assert_eq!(status, AsyncStatus::Error);
            let result = op.get_result();
            assert!(matches!(result, Err(STATUS_UNSUCCESSFUL)));
        }
    }

    #[cfg(not(feature = "driver"))]
    #[test]
    fn pending_future_reports_pending() {
        let _guard = TEST_LOCK.lock().unwrap();
        let op = spawn_async_operation(core::future::pending::<u32>()).expect("spawn async operation");
        unsafe {
            let status = op.get_status().expect("get status");
            assert_eq!(status, AsyncStatus::Started);
            let result = op.get_result();
            assert!(matches!(result, Err(STATUS_PENDING)));
        }
    }

    #[cfg(not(feature = "driver"))]
    #[test]
    fn pending_future_drop_releases_task_ref() {
        let _guard = TEST_LOCK.lock().unwrap();
        ASYNC_OPERATION_DROP_COUNT.store(0, Ordering::Relaxed);
        let op = spawn_async_operation(core::future::pending::<u32>()).expect("spawn async operation");
        drop(op);
        assert_eq!(ASYNC_OPERATION_DROP_COUNT.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn null_out_ptrs_return_error() {
        let _guard = TEST_LOCK.lock().unwrap();
        let op = spawn_async_operation(async { 1u32 }).expect("spawn async operation");
        unsafe {
            let raw = op.as_ptr();
            let vtbl = *(raw as *mut *mut AsyncOperationVtbl<u32>);
            let status = ((*vtbl).get_status)(raw as *mut c_void, core::ptr::null_mut());
            assert_eq!(status, STATUS_UNSUCCESSFUL);
            let status = ((*vtbl).get_result)(raw as *mut c_void, core::ptr::null_mut());
            assert_eq!(status, STATUS_UNSUCCESSFUL);
        }
    }
}
