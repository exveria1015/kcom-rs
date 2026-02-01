#include <iostream>
#include <chrono>
#include <atomic>
#include <memory>
#include <type_traits>

// Windows COM ABI (stdcall) をエミュレート
#if defined(_WIN32)
#define STDMETHODCALLTYPE __stdcall
#else
#define STDMETHODCALLTYPE
#endif

// =========================================================
// 1. Manual COM Async Operation (The "Hardcore" C++ way)
// =========================================================

struct IAsyncOperation {
    virtual unsigned long STDMETHODCALLTYPE AddRef() = 0;
    virtual unsigned long STDMETHODCALLTYPE Release() = 0;
    virtual int STDMETHODCALLTYPE GetStatus(int* status) = 0;
    virtual int STDMETHODCALLTYPE GetResult(int* result) = 0;
};

class AsyncOperationCompleted : public IAsyncOperation {
    std::atomic<unsigned long> ref_count_;
    int result_;

public:
    explicit AsyncOperationCompleted(int result)
        : ref_count_(1), result_(result) {}

    unsigned long STDMETHODCALLTYPE AddRef() override {
        return ref_count_.fetch_add(1, std::memory_order_relaxed) + 1;
    }

    unsigned long STDMETHODCALLTYPE Release() override {
        unsigned long count = ref_count_.fetch_sub(1, std::memory_order_release) - 1;
        if (count == 0) {
            std::atomic_thread_fence(std::memory_order_acquire);
            delete this;
        }
        return count;
    }

    int STDMETHODCALLTYPE GetStatus(int* status) override {
        *status = 1; // Completed
        return 0; // S_OK
    }

    int STDMETHODCALLTYPE GetResult(int* result) override {
        *result = result_;
        return 0; // S_OK
    }
};

struct IMyAsyncOp {
    virtual unsigned long STDMETHODCALLTYPE AddRef() = 0;
    virtual unsigned long STDMETHODCALLTYPE Release() = 0;
    virtual IAsyncOperation* STDMETHODCALLTYPE GetStatusAsync() = 0;
};

class ManualComImpl : public IMyAsyncOp {
    std::atomic<unsigned long> ref_count_;

public:
    ManualComImpl() : ref_count_(1) {}

    unsigned long STDMETHODCALLTYPE AddRef() override {
        return ref_count_.fetch_add(1, std::memory_order_relaxed) + 1;
    }

    unsigned long STDMETHODCALLTYPE Release() override {
        unsigned long count = ref_count_.fetch_sub(1, std::memory_order_release) - 1;
        if (count == 0) {
            std::atomic_thread_fence(std::memory_order_acquire);
            delete this;
        }
        return count;
    }

    IAsyncOperation* STDMETHODCALLTYPE GetStatusAsync() override {
        return new AsyncOperationCompleted(1);
    }
};

// =========================================================
// 2. Modern C++ Implementation (Baseline)
// =========================================================

struct ReadyFuture {
    int value;
};

class ModernImpl {
public:
    int GetStatus() {
        return 1;
    }
};

// =========================================================
// Benchmarking Utilities
// =========================================================

template <class T>
void do_not_optimize(T&& datum) {
    using BaseType = typename std::remove_reference<T>::type;
    volatile BaseType* p = &datum;
    (void)p;
}

template <typename Func>
double measure_ns(const char* name, int iterations, Func func) {
    auto start = std::chrono::high_resolution_clock::now();
    for (int i = 0; i < iterations; ++i) {
        func();
    }
    auto end = std::chrono::high_resolution_clock::now();
    auto duration = std::chrono::duration_cast<std::chrono::nanoseconds>(end - start).count();
    double avg = static_cast<double>(duration) / iterations;

    std::cout << "[" << name << "] Average: " << avg << " ns" << std::endl;
    return avg;
}

int main() {
    const int ITERATIONS = 10000000; // 10M loops

    std::cout << "Running C++ Async Benchmarks (" << ITERATIONS << " iterations)..." << std::endl;
    std::cout << "-----------------------------------------------------" << std::endl;

    // Prepare a COM object.
    IMyAsyncOp* raw_obj = new ManualComImpl();

    // --- Allocation Benchmark ---

    // 1. Manual COM: async operation allocation
    measure_ns("Cpp_AsyncOp_New", ITERATIONS, [raw_obj]() {
        IAsyncOperation* op = raw_obj->GetStatusAsync();
        op->Release();
    });

    // 2. Modern C++: make_shared (ready state)
    measure_ns("Cpp_Make_Shared_Ready", ITERATIONS, []() {
        auto ptr = std::make_shared<ReadyFuture>(ReadyFuture{1});
        do_not_optimize(ptr);
    });

    // --- Dispatch Benchmark ---

    IAsyncOperation* op = raw_obj->GetStatusAsync();

    // 3. Virtual Method Call (COM ABI)
    measure_ns("Cpp_AsyncOp_GetStatus", ITERATIONS, [op]() {
        int status = 0;
        op->GetStatus(&status);
        do_not_optimize(status);
    });

    op->Release();

    // 4. Native direct call
    ModernImpl native;
    measure_ns("Cpp_Native_Call", ITERATIONS, [&native]() {
        do_not_optimize(native.GetStatus());
    });

    raw_obj->Release();

    return 0;
}
