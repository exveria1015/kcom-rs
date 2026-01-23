# kcom

`kcom` is a zero-copy COM runtime for Windows kernel drivers, built for `no_std` Rust. It generates
VTables and shims from Rust traits, minimizing boilerplate for driver authors.

## Features

- **no_std** support for kernel-mode usage
- **Zero-copy layout** (VTable + refcount + Rust struct in one layout)
- **Macro-generated VTables** via `declare_com_interface!`
- **Result -> NTSTATUS** mapping in shims
- **Optional async support (Experimental)** with a blocking executor

## Feature flags

- `async-com`: enables async method support in `declare_com_interface!`
- `async-impl`: enables `async-com` and re-exports `async-trait` as `#[kcom::async_impl]`
- `async-com-kernel`: enables `async-com` and `wdk-sys` (kernel builds)

## Usage (sync interface)

```rust
use core::ffi::c_void;

use kcom::{
    declare_com_interface, impl_com_object, ComImpl, ComInterfaceInfo, ComObject, GUID, IUnknown,
    IUnknownVtbl, NTSTATUS, STATUS_SUCCESS,
};

declare_com_interface! {
    pub trait IFoo: IUnknown {
        const IID: GUID = GUID {
            data1: 0x1234_5678,
            data2: 0x1234,
            data3: 0x5678,
            data4: [0x90, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67],
        };

        fn ping(&self, value: u32) -> NTSTATUS;
    }
}

struct Foo;

impl IFoo for Foo {
    fn ping(&self, _value: u32) -> NTSTATUS {
        STATUS_SUCCESS
    }
}

impl ComImpl<IFooVtbl> for Foo {
    const VTABLE: &'static IFooVtbl = &IFooVtbl {
        parent: *<Foo as ComImpl<IUnknownVtbl>>::VTABLE,
        ping: shim_IFoo_ping::<Foo>,
    };

    fn query_interface(&self, this: *mut c_void, riid: &GUID) -> Option<*mut c_void> {
        if *riid == <IFooInterface as ComInterfaceInfo>::IID {
            Some(this)
        } else {
            <Foo as ComImpl<IUnknownVtbl>>::query_interface(self, this, riid)
        }
    }
}

impl_com_object!(Foo, IFooVtbl);

fn main() {
    let raw = Foo::new_com(Foo);

    unsafe {
        let vtbl = *(raw as *mut *const IFooVtbl);
        let status = ((*vtbl).ping)(raw, 42);
        assert_eq!(status, STATUS_SUCCESS);

        ComObject::<Foo, IFooVtbl>::shim_release(raw);
    }
}
```

## Usage (async interface)

> **Experimental**: Async support is still evolving and may change in future releases.

Enable async support for user-mode tests (no WDK required):

```
cargo run --example declare_com_interface_async --features async-com
```

For kernel builds (WDK + driver_model cfg), enable:

```
cargo build --features async-com-kernel
```

Declare async methods in the interface:

```rust
use kcom::{declare_com_interface, NTSTATUS};

declare_com_interface! {
    pub trait IMyDriver: IUnknown {
        const IID: GUID = /* ... */;

        async fn init(&self) -> NTSTATUS;
    }
}
```

### Implementing async methods

Without sugar, you must return a boxed future:

```rust
use core::future::Future;
use core::pin::Pin;

impl IMyDriver for MyDriver {
    fn init(&self) -> Pin<Box<dyn Future<Output = NTSTATUS> + Send + '_>> {
        Box::pin(async move { STATUS_SUCCESS })
    }
}
```

With `async-impl` enabled, use `#[kcom::async_impl]` (powered by `async-trait`):

```rust
use kcom::async_impl;

#[async_impl]
impl IMyDriver for MyDriver {
    async fn init(&self) -> NTSTATUS {
        STATUS_SUCCESS
    }
}
```

### Async blocking caveat

Async COM shims block the calling thread while polling the future. Avoid awaiting re-entrant
COM calls on the same thread (deadlock risk). Design async methods to complete without needing
the caller thread to pump messages.

## Kernel safety notes

The blocking executor waits in kernel mode. For safe usage:

1. **IRQL guard**: calling at `DISPATCH_LEVEL` or higher is rejected (debug assertion)
2. **Watchdog**: debug-only timeout detects deadlocks
3. **Stack safety**: wakers use heap-owned events (`Arc`) and futures are heap-pinned via `Box::pin`

Each async call allocates a boxed future. This is suitable for control paths (init, create, etc.).
Real-time / hot paths may require allocation-free designs.

## License

Licensed under either of

- Apache License, Version 2.0
- MIT License

at your option.
