use core::ffi::c_void;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

use std::boxed::Box;

use kcom::{
    declare_com_interface, impl_com_object, ComImpl, ComInterfaceInfo, ComObject, GUID, IUnknownVtbl,
    NTSTATUS, STATUS_SUCCESS,
};

declare_com_interface! {
    /// Async COM interface example (kernel-only).
    pub trait IAsyncFoo: IUnknown {
        const IID: GUID = GUID {
            data1: 0xABCD_1234,
            data2: 0x5678,
            data3: 0x90AB,
            data4: [0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67, 0x89, 0x0A],
        };

        async fn init(&self, value: u32) -> NTSTATUS;
    }
}

struct YieldOnce {
    value: NTSTATUS,
    yielded: bool,
}

impl Future for YieldOnce {
    type Output = NTSTATUS;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.yielded {
            Poll::Ready(self.value)
        } else {
            self.yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

struct AsyncFoo;

impl IAsyncFoo for AsyncFoo {
    fn init(&self, _value: u32) -> Pin<Box<dyn Future<Output = NTSTATUS> + Send + '_>> {
        Box::pin(YieldOnce {
            value: STATUS_SUCCESS,
            yielded: false,
        })
    }
}

impl ComImpl<IAsyncFooVtbl> for AsyncFoo {
    const VTABLE: &'static IAsyncFooVtbl = &IAsyncFooVtbl {
        parent: *<AsyncFoo as ComImpl<IUnknownVtbl>>::VTABLE,
        init: shim_IAsyncFoo_init::<AsyncFoo>,
    };

    fn query_interface(&self, riid: &GUID) -> Option<*mut c_void> {
        if *riid == <IAsyncFooInterface as ComInterfaceInfo>::IID {
            Some(self as *const AsyncFoo as *mut c_void)
        } else {
            <AsyncFoo as ComImpl<IUnknownVtbl>>::query_interface(self, riid)
        }
    }
}

impl_com_object!(AsyncFoo, IAsyncFooVtbl);

fn main() {
    let raw = AsyncFoo::new_com(AsyncFoo);

    unsafe {
        let vtbl = *(raw as *mut *const IAsyncFooVtbl);
        let status = ((*vtbl).init)(raw, 123);
        assert_eq!(status, STATUS_SUCCESS);

        ComObject::<AsyncFoo, IAsyncFooVtbl>::shim_release(raw);
    }
}
