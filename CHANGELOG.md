# Changelog

## 0.4.0 - 2026-01-24
- Add `impl_query_interface!` macro for multi-interface `QueryInterface` implementations.
- Add `ComRc` smart pointer for automatic AddRef/Release handling.
- Make kernel `block_on` IRQL guard always enforced.
- Add `kernel-unicode` feature with `UNICODE_STRING` helpers.
- Document new APIs in README.
