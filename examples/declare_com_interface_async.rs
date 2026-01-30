#[cfg(feature = "async-com")]
mod async_example {
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Context, Poll};

    use kcom::{
        declare_com_interface, impl_com_interface, impl_com_object, pin_init, ComObject,
        GlobalAllocator, GUID, IUnknownVtbl, InitBox, InitBoxTrait, NTSTATUS, STATUS_SUCCESS,
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

    unsafe impl IAsyncFoo for AsyncFoo {
        type InitFuture<'a> = YieldOnce;
        type Allocator = GlobalAllocator;

        fn init(&self, _value: u32) -> impl InitBoxTrait<Self::InitFuture<'_>, Self::Allocator, NTSTATUS> {
            InitBox::new(
                GlobalAllocator,
                pin_init!(YieldOnce {
                    value: STATUS_SUCCESS,
                    yielded: false,
                }),
            )
        }
    }

    impl_com_interface! {
        impl AsyncFoo: IAsyncFoo {
            parent = IUnknownVtbl,
            methods = [init],
        }
    }

    impl_com_object!(AsyncFoo, IAsyncFooVtbl);

    pub fn run() {
        let raw = AsyncFoo::new_com(AsyncFoo).unwrap();

        unsafe {
            let vtbl = *(raw as *mut *const IAsyncFooVtbl);
            let status = ((*vtbl).init)(raw, 123);
            assert_eq!(status, STATUS_SUCCESS);

            ComObject::<AsyncFoo, IAsyncFooVtbl>::shim_release(raw);
        }
    }
}

#[cfg(feature = "async-com")]
fn main() {
    async_example::run();
}

#[cfg(not(feature = "async-com"))]
fn main() {}
