# Benchmarks

Benchmark sources live in `benches/`:

- `comparison.rs` / `comparison.cpp` (sync comparison)
- `comparison_async.rs` / `comparison_async.cpp` (async comparison)
- `async_benchmark.rs` (criterion benchmark, async-com feature)

## Running (Rust)

```text
cargo bench --bench comparison
cargo bench --bench comparison_async --features async-com
```

## Running (C++)

The C++ benchmarks build to `benches/*.exe`. Run from the repo root:

```text
.\benches\comparison.exe
.\benches\comparison_async.exe
```

## Interpretation guidance

- Always report the environment: CPU, power plan, build profile.
- Consider subtracting empty-loop overhead to get adjusted values.
- For kernel-oriented paths (DPC or WDK calls), document any kernel API cost
  that dominates the timing.

Recorded numbers live in `Benchmark.md`.

