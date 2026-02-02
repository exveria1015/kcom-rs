# Troubleshooting

## LNK2019: unresolved external symbol DriverEntry

When building driver features in user-mode tests, link can fail with:

- `WdfDriverEntry.lib(stub.obj) : error LNK2019: unresolved external symbol DriverEntry`

Use the `driver-test-stub` feature to provide a stub `DriverEntry`.

## Miri not available on stable

`cargo miri` requires the nightly toolchain:

```text
cargo +nightly miri test -p kcom-tests --features async-com
```

## Access denied in target/ (Windows)

If builds fail with `os error 5` (access denied):

- Close any process that might be holding files in `target/`
- Temporarily disable antivirus real-time scanning for the repo
- Retry the build

## Unexpected cfg warnings

`kcom` uses a build-time cfg (`kcom_strict_provenance`) in nightly/Miri.
The build script advertises it via `rustc-check-cfg`; if warnings persist,
ensure `build.rs` is executed and not blocked.

## Miri: integer-to-pointer cast warnings

These warnings indicate provenance-sensitive casts. Use the strict provenance
path (nightly/Miri auto-enabled, or `strict-provenance` feature) for validation.

## Kernel unicode tests failing

The `kernel-unicode` feature expects kernel UNICODE_STRING types. In host/Miri
builds, the crate provides a stub type; ensure the feature set matches the
intended environment.

