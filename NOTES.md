# Notes

## Provenance / Miri

- Default build stores async guard pointers as `usize` for minimal overhead; Miri warns about `integer-to-pointer cast` in async paths.
- Feature `strict-provenance` switches those guards to typed pointers (no integer casts) to quiet Miri and tighten provenance in safety checks.
- Policy: keep default for kernel performance, enable `strict-provenance` for Miri/CI validation runs (e.g. `cargo +nightly miri test -p kcom-tests --features "async-com strict-provenance"`).
