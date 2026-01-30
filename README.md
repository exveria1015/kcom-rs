# kcom

`kcom` is a zero-copy COM runtime for Windows kernel drivers, built for `no_std` Rust. It generates
VTables and shims from Rust traits, minimizing boilerplate for driver authors.

## Features

- **no_std** support for kernel-mode usage
- **Zero-copy layout** (VTable + refcount + Rust struct in one layout)
- **Macro-generated VTables** via `declare_com_interface!`
- **Result -> NTSTATUS** mapping in shims
- **Optional async support (Experimental)** with a blocking executor
- **QueryInterface helper macro** for multi-interface support
- **Reference-counted ComRc** smart pointer for client-side COM usage
- **Kernel Unicode helpers** for `UNICODE_STRING`

## Feature flags

- `async-com`: enables async method support in `declare_com_interface!`
- `async-impl`: enables `async-com` and re-exports `async-trait` as `#[kcom::async_impl]`
- `async-com-kernel`: enables `async-com` and `wdk-sys` (kernel builds)
- `kernel-unicode`: enables `UNICODE_STRING` helpers (requires `wdk-sys`)

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

    impl_query_interface! {
        Self,
        this,
        riid,
        [IFoo],
        fallback = IUnknownVtbl
    }
}

impl_com_object!(Foo, IFooVtbl);

fn main() {
    let com = Foo::new_com_rc::<IFooRaw>(Foo).unwrap();

    unsafe {
        let ptr = com.as_ptr();
        let vtbl = *(ptr as *mut *const IFooVtbl);
        let status = ((*vtbl).ping)(ptr as *mut _, 42);
        assert_eq!(status, STATUS_SUCCESS);
    }
}
```

When supporting additional interfaces, return explicit tear-off or aggregated pointers for
non-primary interfaces so the returned pointer’s vtable matches the requested IID.

Use `new_com_rc::<IFooRaw>` (or `new_com_rc_in`) to receive a `ComRc` that owns the initial
reference. `new_com` still returns a raw pointer with refcount 1.

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

Without sugar, you must return an initializer (InitBox) that the shim will allocate:

```rust
use core::future::{ready, Future, Ready};

use kcom::{GlobalAllocator, InitBox, pin_init};

// Async interfaces are `unsafe` to implement because they use a blocking executor.
unsafe impl IMyDriver for MyDriver {
    type InitFuture<'a> = Ready<NTSTATUS>;
    type Allocator = GlobalAllocator;

    fn init(&self) -> impl InitBoxTrait<Self::InitFuture<'_>, Self::Allocator, NTSTATUS> {
        InitBox::new(GlobalAllocator, pin_init!(ready(STATUS_SUCCESS)))
    }
}
```

`async-impl` (powered by `async-trait`) does not support returning initializer types, so for
`async-com` you must implement the InitBox pattern manually.

```rust
use kcom::async_impl;

// #[async_impl]
// unsafe impl IMyDriver for MyDriver {
//     async fn init(&self) -> NTSTATUS {
//         STATUS_SUCCESS
//     }
// }
```

### Async blocking caveat

Async COM shims block the calling thread while polling the future. Avoid awaiting re-entrant
COM calls on the same thread (deadlock risk). Design async methods to complete without needing
the caller thread to pump messages.

In kernel mode, the blocking executor is `unsafe` to call. You must uphold IRQL and deadlock
requirements (see below).

## Kernel safety notes

The blocking executor waits in kernel mode. For safe usage:

1. **IRQL guard**: calling at `DISPATCH_LEVEL` or higher is rejected (always enforced)
2. **Watchdog**: debug-only timeout detects deadlocks
3. **Stack safety**: wakers use heap-owned events (`Arc`); the executor pins on the stack
4. **Deadlock safety**: do not call while holding spinlocks or resources needed by the future

Each async call still requires a heap allocation (KBox) from the implementation. This is suitable
for control paths (init, create, etc.). Real-time / hot paths may require allocation-free designs.

### True async via WorkItems

When you must avoid blocking (IRQL > APC_LEVEL), queue the future onto a WorkItem and return
`STATUS_PENDING`. KMDF and WDM helpers are provided in the executor module:

```rust
#[cfg(driver_model__driver_type = "KMDF")]
unsafe {
    kcom::executor::spawn_work_item_kmdf(device, future, completion_callback)?;
}

#[cfg(driver_model__driver_type = "WDM")]
unsafe {
    kcom::executor::spawn_work_item_wdm(device, queue, future, completion_callback)?;
}
```

> **WDM注意**: ドライバの Unload と WorkItem 実行の競合を避けるため、
> `IoAcquireRemoveLock` 等で寿命管理を行ってください。

## Client-side COM pointers

Use `ComRc<T>` to manage COM references safely (AddRef/Release).

```rust
use kcom::ComRc;

// SAFETY: `raw` must be a valid COM interface pointer.
let com_ref = unsafe { ComRc::from_raw_addref(raw).unwrap() };
let raw_again = com_ref.as_ptr();
```

## Aggregation (non-delegating IUnknown)

`ComObject::new_aggregated` returns a **non-delegating IUnknown** pointer for use by the outer
object. The outer object owns the lifetime of the inner object through this pointer.

Calls made through interfaces returned by `QueryInterface` are **delegated** to the outer
unknown, while the non-delegating pointer manages the inner refcount directly.

```rust
use kcom::{ComObject, IUnknownVtbl};

// SAFETY: outer_unknown must be a valid IUnknown vtable from the outer object.
let non_delegating = ComObject::<Inner, IUnknownVtbl>::new_aggregated(inner, outer_unknown);
```

## Unicode helpers (kernel)

Enable the `kernel-unicode` feature to construct and read `UNICODE_STRING` values.

```rust
use kcom::OwnedUnicodeString;

let name = OwnedUnicodeString::new("\\Device\\MyDriver").unwrap();
let unicode = name.as_unicode();
```

Compile-time `UNICODE_STRING` literals can be built with `kstr!`:

```rust
use kcom::kstr;

let name = kstr!("\\Device\\MyDriver");
// name: &'static UNICODE_STRING
```

## License

Licensed under either of

- Apache License, Version 2.0
- MIT License

at your option.
