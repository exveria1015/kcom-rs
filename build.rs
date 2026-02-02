use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(kcom_strict_provenance)");
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let is_nightly = Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|version| version.contains("nightly"))
        .unwrap_or(false);

    if is_nightly || env::var("CARGO_CFG_MIRI").is_ok() {
        println!("cargo:rustc-cfg=kcom_strict_provenance");
    }
}
