#[cfg(feature = "async-com")]
use kcom::{
    declare_com_interface, impl_com_interface, impl_com_object, pin_init, AsyncOperationRaw,
    AsyncStatus, ComObject, ComRc, GlobalAllocator, GUID, IUnknownVtbl, InitBox, InitBoxTrait,
    NTSTATUS,
};
#[cfg(feature = "async-com")]
use std::hint::black_box;
#[cfg(feature = "async-com")]
use std::time::Instant;

// =========================================================
// 1. Interface Definition (Async)
// =========================================================

#[cfg(feature = "async-com")]
declare_com_interface! {
    pub trait IMyAsyncOp: IUnknown {
        const IID: GUID = GUID {
            data1: 0x2222_3333, data2: 0x4444, data3: 0x5555,
            data4: [0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD],
        };
        async fn get_status(&self) -> i32;
    }
}

// =========================================================
// 2. kcom Implementation (Async)
// =========================================================

#[cfg(feature = "async-com")]
struct MyAsyncImpl;

#[cfg(feature = "async-com")]
unsafe impl IMyAsyncOp for MyAsyncImpl {
    type GetStatusFuture = core::future::Ready<i32>;
    type Allocator = GlobalAllocator;

    fn get_status(&self) -> impl InitBoxTrait<Self::GetStatusFuture, Self::Allocator, NTSTATUS> {
        InitBox::new(GlobalAllocator, pin_init!(core::future::ready(1)))
    }
}

#[cfg(feature = "async-com")]
impl_com_interface! {
    impl MyAsyncImpl: IMyAsyncOp {
        parent = IUnknownVtbl,
        methods = [get_status],
    }
}

#[cfg(feature = "async-com")]
impl_com_object!(MyAsyncImpl, IMyAsyncOpVtbl);

// =========================================================
// 3. Standard Rust Implementation (Baseline)
// =========================================================

#[cfg(feature = "async-com")]
struct ModernImpl;

#[cfg(feature = "async-com")]
impl ModernImpl {
    fn get_status(&self) -> i32 {
        1
    }
}

// =========================================================
// Benchmarking Utilities
// =========================================================

#[cfg(feature = "async-com")]
fn measure_ns<F>(name: &str, iterations: u64, mut func: F) -> f64
where
    F: FnMut(),
{
    let start = Instant::now();
    for _ in 0..iterations {
        func();
    }
    let duration = start.elapsed();
    let total_ns = duration.as_nanos() as f64;
    let avg = total_ns / iterations as f64;

    println!("[{}] Average: {:.5} ns", name, avg);
    avg
}

#[cfg(feature = "async-com")]
fn main() {
    const ITERATIONS: u64 = 10_000_000; // 10M loops

    println!("Running Rust (kcom) Async Benchmarks ({} iterations)...", ITERATIONS);
    println!("-----------------------------------------------------");

    // Prepare a COM object.
    let raw_obj = MyAsyncImpl::new_com(MyAsyncImpl).unwrap();
    let vtbl = unsafe { *(raw_obj as *mut *const IMyAsyncOpVtbl) };

    // --- Allocation Benchmark ---

    // 1. kcom: async method -> AsyncOperation allocation
    measure_ns("Rust_kcom_AsyncOp_New", ITERATIONS, || {
        let op = unsafe { ((*vtbl).get_status)(raw_obj) };
        if op.is_null() {
            return;
        }
        unsafe {
            let op = ComRc::<AsyncOperationRaw<i32>>::from_raw_unchecked(op);
            black_box(op.as_ptr());
        }
    });

    // 2. Standard Rust: Box::pin(ready)
    measure_ns("Rust_Box_Pin_Ready", ITERATIONS, || {
        let future = core::future::ready(1i32);
        let boxed = Box::pin(future);
        black_box(boxed);
    });

    // --- Dispatch Benchmark ---

    // Prepare a completed async operation.
    let op = unsafe {
        let ptr = ((*vtbl).get_status)(raw_obj);
        if ptr.is_null() {
            return;
        }
        ComRc::<AsyncOperationRaw<i32>>::from_raw_unchecked(ptr)
    };
    let op_ptr = op.as_ptr();

    // 3. VTable call on AsyncOperation::get_status
    measure_ns("Rust_kcom_AsyncOp_GetStatus", ITERATIONS, || {
        unsafe {
            let mut status = AsyncStatus::Started;
            let _ = ((*(*op_ptr).lpVtbl).get_status)(
                op_ptr as *mut core::ffi::c_void,
                &mut status,
            );
            black_box(status);
        }
    });

    // 4. Standard Rust: direct method call
    let native = ModernImpl;
    measure_ns("Rust_Native_Call", ITERATIONS, || {
        black_box(native.get_status());
    });

    // Cleanup.
    unsafe {
        ComObject::<MyAsyncImpl, IMyAsyncOpVtbl>::shim_release(raw_obj);
    }
}

#[cfg(not(feature = "async-com"))]
fn main() {
    eprintln!("This benchmark requires the `async-com` feature. Run with `--features async-com`." );
}
