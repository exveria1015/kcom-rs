# トラブルシューティング

## LNK2019: DriverEntry が未解決

`driver` feature をユーザーモードでリンクすると以下が出ることがあります:

- `WdfDriverEntry.lib(stub.obj) : error LNK2019: unresolved external symbol DriverEntry`

`driver-test-stub` feature を付けて `DriverEntry` のスタブを提供してください。

## Miri は stable で使えない

`cargo miri` は nightly が必要です:

```text
cargo +nightly miri test -p kcom-tests --features async-com
```

## target 配下へのアクセス拒否 (Windows)

`os error 5` が出る場合:

- `target/` を掴んでいるプロセスがないか確認
- ウイルス対策のリアルタイムスキャンを一時停止
- 再ビルド

## unexpected cfg 警告

`kcom_strict_provenance` のような build cfg に対する警告です。
`build.rs` が動いているか確認し、`rustc-check-cfg` が出力されることを
確認してください。

## Miri の integer-to-pointer cast 警告

provenance に関する警告です。nightly/Miri では strict 側が自動有効化されます。
明示的に有効化する場合は `strict-provenance` feature を使います。

## kernel-unicode の失敗

`kernel-unicode` はカーネルの `UNICODE_STRING` に依存します。
ホスト/Miri ではスタブ型を使用しているため、想定した feature 構成か
確認してください。

