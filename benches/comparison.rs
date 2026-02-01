use kcom::{
    declare_com_interface, impl_com_interface, impl_com_object, ComObject, GUID, IUnknownVtbl,
    NTSTATUS, STATUS_SUCCESS,
};
use std::hint::black_box;
use std::time::Instant;

// =========================================================
// 1. Interface Definition
// =========================================================

declare_com_interface! {
    pub trait IMyAsyncOp: IUnknown {
        const IID: GUID = GUID {
            data1: 0x1111_2222, data2: 0x3333, data3: 0x4444,
            data4: [0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC],
        };
        // C++: virtual int GetStatus(int* status) = 0;
        fn get_status(&self, status: &mut i32) -> NTSTATUS;
    }
}

// =========================================================
// 2. kcom Implementation (Corresponds to ManualComImpl)
// =========================================================

struct MyImpl;

impl IMyAsyncOp for MyImpl {
    fn get_status(&self, status: &mut i32) -> NTSTATUS {
        *status = 1;
        STATUS_SUCCESS
    }
}

impl_com_interface! {
    impl MyImpl: IMyAsyncOp {
        parent = IUnknownVtbl,
        methods = [get_status],
    }
}

impl_com_object!(MyImpl, IMyAsyncOpVtbl);

// =========================================================
// 3. Standard Rust Implementation (Corresponds to ModernImpl)
// =========================================================

struct ModernImpl;

impl ModernImpl {
    fn get_status(&self, status: &mut i32) -> i32 {
        *status = 1;
        0
    }
}

// =========================================================
// Benchmarking Utilities
// =========================================================

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

fn main() {
    const ITERATIONS: u64 = 10_000_000; // 10M loops

    println!("Running Rust (kcom) Benchmarks ({} iterations)...", ITERATIONS);
    println!("-----------------------------------------------------");

    // --- Allocation Benchmark ---

    // 1. kcom: new + RefCount Init (Corresponds to Cpp_Manual_New)
    measure_ns("Rust_kcom_New", ITERATIONS, || {
        // C++: new ManualComImpl() -> Release()
        // Rust: ComObject::new() -> shim_release()
        // ※ new_com は *mut c_void を返す (Raw pointer)
        let ptr = MyImpl::new_com(MyImpl).unwrap();
        unsafe {
            ComObject::<MyImpl, IMyAsyncOpVtbl>::shim_release(ptr);
        }
    });

    // 2. Standard Rust: Box::new (Corresponds to Cpp_Make_Shared)
    measure_ns("Rust_Box_New", ITERATIONS, || {
        // C++: std::make_shared<ModernImpl>()
        // Rust: Box::new(ModernImpl)
        let ptr = Box::new(ModernImpl);
        black_box(ptr);
    });

    // --- Dispatch Benchmark ---

    // 準備: Raw pointer を作成
    let raw_void = MyImpl::new_com(MyImpl).unwrap();
    let raw_ptr = raw_void as *mut IMyAsyncOpRaw;

    // 3. Virtual Method Call (Corresponds to Cpp_Virtual_Call)
    measure_ns("Rust_kcom_Call", ITERATIONS, || {
        let mut status = 0;
        unsafe {
            // C++: raw_obj->GetStatus(&status)
            let vtbl = (*raw_ptr).lpVtbl;
            let _ret = ((*vtbl).get_status)(
                raw_ptr as *mut core::ffi::c_void,
                &mut status,
            );
            black_box(status);
        }
    });

    // クリーンアップ
    unsafe {
        ComObject::<MyImpl, IMyAsyncOpVtbl>::shim_release(raw_void);
    }
}