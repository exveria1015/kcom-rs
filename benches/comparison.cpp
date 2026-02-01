#include <iostream>
#include <vector>
#include <chrono>
#include <atomic>
#include <memory>
#include <thread>
#include <type_traits> // 追加

// Windows COM ABI (stdcall) をエミュレート
#if defined(_WIN32)
#define STDMETHODCALLTYPE __stdcall
#else
#define STDMETHODCALLTYPE
#endif

// =========================================================
// 1. Manual COM Implementation (The "Hardcore" C++ way)
// =========================================================

struct IMyAsyncOp {
    virtual unsigned long STDMETHODCALLTYPE AddRef() = 0;
    virtual unsigned long STDMETHODCALLTYPE Release() = 0;
    virtual int STDMETHODCALLTYPE GetStatus(int* status) = 0;
};

class ManualComImpl : public IMyAsyncOp {
    std::atomic<unsigned long> ref_count_;
    int result_;

public:
    ManualComImpl() : ref_count_(1), result_(0) {}

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
};

// =========================================================
// 2. Modern C++ Implementation (std::shared_ptr)
// =========================================================

class ModernImpl {
public:
    int GetStatus(int* status) {
        *status = 1;
        return 0;
    }
};

// =========================================================
// Benchmarking Utilities
// =========================================================

// 修正: コンパイラの最適化によるループ削除を防ぐ
template <class T>
void do_not_optimize(T&& datum) {
    // 参照型を削除してポインタを作成できるようにする
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
    double avg = (double)duration / iterations;
    
    std::cout << "[" << name << "] Average: " << avg << " ns" << std::endl;
    return avg;
}

int main() {
    const int ITERATIONS = 10000000; // 10M loops

    std::cout << "Running C++ Benchmarks (" << ITERATIONS << " iterations)..." << std::endl;
    std::cout << "-----------------------------------------------------" << std::endl;

    // --- Allocation Benchmark ---

    // 1. Manual COM: new + RefCount Init
    measure_ns("Cpp_Manual_New", ITERATIONS, []() {
        IMyAsyncOp* obj = new ManualComImpl();
        // すぐに捨てる
        obj->Release(); 
    });

    // 2. Modern C++: make_shared (Single Allocation)
    measure_ns("Cpp_Make_Shared", ITERATIONS, []() {
        auto ptr = std::make_shared<ModernImpl>();
        do_not_optimize(ptr);
    });

    // --- Dispatch Benchmark ---

    // 準備
    IMyAsyncOp* raw_obj = new ManualComImpl();
    
    // 3. Virtual Method Call (COM ABI)
    measure_ns("Cpp_Virtual_Call", ITERATIONS, [raw_obj]() {
        int status;
        raw_obj->GetStatus(&status);
        do_not_optimize(status);
    });

    raw_obj->Release();

    return 0;
}