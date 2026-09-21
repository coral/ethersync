//! Optional Swift compiler invocation; no Cargo recursion or script language.
use std::{path::Path, process::Command};
fn run(command: &mut Command) {
    assert!(
        command
            .status()
            .expect("Swift compiler is required for ETHERSYNC_SDK_SWIFT_DYLIB")
            .success(),
        "Swift SDK compilation failed"
    );
}
pub fn build(sdk: &Path, native: bool, rust_target: &str) {
    let target = if rust_target.starts_with("aarch64-") {
        "arm64-apple-macosx13.0"
    } else {
        "x86_64-apple-macosx13.0"
    };
    let out = sdk.join("swift-dylib");
    std::fs::create_dir_all(&out).unwrap();
    for (module, files, dependency) in [
        (
            "EthersyncSys",
            vec![
                sdk.join("swift/SwiftBridgeCore.swift"),
                sdk.join("swift/Ethersync.swift"),
            ],
            "ethersync_bindings",
        ),
        (
            "Ethersync",
            vec![sdk.join("swift-client/Ethersync.swift")],
            "EthersyncSys",
        ),
    ] {
        run(Command::new("swiftc")
            .args([
                "-target",
                target,
                "-emit-library",
                "-emit-module",
                "-module-name",
                module,
            ])
            .args(["-I"])
            .arg(sdk.join("swift-c"))
            .arg("-I")
            .arg(&out)
            .arg("-L")
            .arg(sdk.join("lib"))
            .arg("-L")
            .arg(&out)
            .arg(format!("-l{dependency}"))
            .arg("-emit-module-path")
            .arg(out.join(format!("{module}.swiftmodule")))
            .args(["-Xlinker", "-install_name", "-Xlinker"])
            .arg(format!("@rpath/lib{module}.dylib"))
            .arg("-o")
            .arg(out.join(format!("lib{module}.dylib")))
            .args(files));
    }
    let mut smoke = Command::new("swiftc");
    smoke
        .args(["-target", target])
        .arg("-I")
        .arg(&out)
        .arg("-I")
        .arg(sdk.join("swift-c"))
        .arg("-L")
        .arg(&out)
        .arg("-lEthersync")
        .args([
            "-Xlinker",
            "-rpath",
            "-Xlinker",
            "@executable_path",
            "-Xlinker",
            "-rpath",
            "-Xlinker",
            "@executable_path/../lib",
        ])
        .arg(sdk.join("swift-example/main.swift"))
        .arg("-o")
        .arg(out.join("EthersyncSmoke"));
    if native {
        smoke.args(["-D", "ETHERSYNC_NATIVE"]);
    }
    run(&mut smoke);
}
