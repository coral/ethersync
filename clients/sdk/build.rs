mod apple;
mod swift_dynamic;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if matches!(
            entry.file_name().to_str(),
            Some("target" | "dist" | "bin" | "obj" | ".build" | ".DS_Store")
        ) {
            continue;
        }
        if path.is_dir() {
            copy_tree(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(path, to.join(entry.file_name())).unwrap();
        }
    }
}
fn main() {
    for key in [
        "TIDKOD_SDK_OUT",
        "TIDKOD_SDK_ARTIFACTS",
        "TIDKOD_SDK_VARIANT",
        "TIDKOD_SDK_SWIFT_DYLIB",
        "TIDKOD_SDK_APPLE_ARTIFACTS",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let Some(destination) = env::var_os("TIDKOD_SDK_OUT") else {
        return;
    };
    let destination = PathBuf::from(destination);
    if let Some(artifacts) = env::var_os("TIDKOD_SDK_APPLE_ARTIFACTS") {
        apple::package(&artifacts, &destination);
        return;
    }
    println!("cargo:rerun-if-changed=templates/CMakeLists.txt");
    let artifacts = PathBuf::from(
        env::var_os("TIDKOD_SDK_ARTIFACTS")
            .expect("set TIDKOD_SDK_ARTIFACTS to the completed Cargo profile directory"),
    );
    let variant = env::var("TIDKOD_SDK_VARIANT").unwrap_or_else(|_| "native".into());
    assert!(variant == "native" || variant == "core");
    let generated = artifacts.join("tidkod-generated").join(&variant);
    let target = fs::read_to_string(generated.join("target.txt"))
        .expect("build the requested binding variant first");
    println!("cargo:rerun-if-changed={}", generated.display());
    let sdk = destination.join(format!("tidkod-{variant}-{target}"));
    fs::create_dir_all(sdk.join("lib")).unwrap();
    copy_tree(&generated, &sdk.join("include"));
    let mut copied = false;
    for name in [
        "libtidkod_bindings.a",
        "libtidkod_bindings.dylib",
        "libtidkod_bindings.so",
        "tidkod_bindings.lib",
        "tidkod_bindings.dll",
        "tidkod_bindings.dll.lib",
    ] {
        let from = artifacts.join(name);
        if from.exists() {
            println!("cargo:rerun-if-changed={}", from.display());
            fs::copy(from, sdk.join("lib").join(name)).unwrap();
            copied = true;
        }
    }
    assert!(
        copied,
        "no completed binding library found in {}",
        artifacts.display()
    );
    let repo = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    for path in [
        ".cargo",
        "protocol",
        "native",
        "clients/bindings",
        "clients/csharp",
        "clients/wasm",
        "clients/sdk",
        "Cargo.toml",
        "Cargo.lock",
    ] {
        println!("cargo:rerun-if-changed={}", repo.join(path).display());
    }
    copy_tree(
        &repo.join("clients/bindings/examples"),
        &sdk.join("examples"),
    );
    fs::copy(
        repo.join("clients/bindings/README.md"),
        sdk.join("README.md"),
    )
    .unwrap();
    fs::write(
        sdk.join("CMakeLists.txt"),
        include_str!("templates/CMakeLists.txt")
            .replace("@NATIVE@", if variant == "native" { "ON" } else { "OFF" }),
    )
    .unwrap();
    if target.contains("apple-darwin") && sdk.join("include/Tidkod.swift").exists() {
        let headers = sdk.join("swift-c");
        fs::create_dir_all(&headers).unwrap();
        for name in ["SwiftBridgeCore.h", "TidkodSwift.h", "BridgingHeader.h"] {
            fs::copy(sdk.join("include").join(name), headers.join(name)).unwrap();
        }
        fs::write(
            headers.join("module.modulemap"),
            "module RustTidkod { header \"BridgingHeader.h\" export * }\n",
        )
        .unwrap();
        let binary = sdk.join("RustTidkod.xcframework");
        if binary.exists() {
            fs::remove_dir_all(&binary).unwrap();
        }
        // A static XCFramework is an archive, headers, and a target manifest.
        // Assemble it directly so packaging needs only Rust, not Xcode plug-ins.
        let architecture = if target.starts_with("aarch64-") {
            "arm64"
        } else if target.starts_with("x86_64-") {
            "x86_64"
        } else {
            panic!("unsupported macOS SDK architecture")
        };
        let identifier = format!("macos-{architecture}");
        let slice = binary.join(&identifier);
        fs::create_dir_all(&slice).unwrap();
        fs::copy(
            sdk.join("lib/libtidkod_bindings.a"),
            slice.join("libtidkod_bindings.a"),
        )
        .unwrap();
        copy_tree(&headers, &slice.join("Headers"));
        fs::write(binary.join("Info.plist"),format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundlePackageType</key><string>XFWK</string>
<key>XCFrameworkFormatVersion</key><string>1.0</string>
<key>AvailableLibraries</key><array><dict>
<key>LibraryIdentifier</key><string>{identifier}</string>
<key>LibraryPath</key><string>libtidkod_bindings.a</string>
<key>HeadersPath</key><string>Headers</string>
<key>SupportedPlatform</key><string>macos</string>
<key>SupportedArchitectures</key><array><string>{architecture}</string></array>
</dict></array></dict></plist>
"#)).unwrap();
        let swift = sdk.join("swift");
        fs::create_dir_all(&swift).unwrap();
        for name in ["SwiftBridgeCore.swift", "Tidkod.swift"] {
            let text = fs::read_to_string(sdk.join("include").join(name)).unwrap();
            fs::write(swift.join(name), format!("import RustTidkod\n{text}")).unwrap();
        }
        fs::create_dir_all(sdk.join("swift-client")).unwrap();
        fs::copy(
            sdk.join("include/TidkodClient.swift"),
            sdk.join("swift-client/Tidkod.swift"),
        )
        .unwrap();
        fs::create_dir_all(sdk.join("swift-example")).unwrap();
        let example =
            fs::read_to_string(repo.join("clients/bindings/examples/client.swift")).unwrap();
        fs::write(
            sdk.join("swift-example/main.swift"),
            format!("import Tidkod\n{example}"),
        )
        .unwrap();
        fs::write(sdk.join("Package.swift"),format!(r#"// swift-tools-version: 5.9
import PackageDescription
let package = Package(name: "Tidkod", platforms: [.macOS(.v13)], products: [.library(name: "Tidkod", targets: ["Tidkod"])], targets: [
    .binaryTarget(name: "RustTidkod", path: "RustTidkod.xcframework"),
    .target(name: "TidkodSys", dependencies: ["RustTidkod"], path: "swift", linkerSettings: [.linkedLibrary("c++"), .linkedFramework("Security"), .linkedFramework("SystemConfiguration"), .linkedFramework("CoreFoundation")]),
    .target(name: "Tidkod", dependencies: ["TidkodSys"], path: "swift-client"),
    .executableTarget(name: "TidkodSmoke", dependencies: ["Tidkod"], path: "swift-example", swiftSettings: [{}])
])
"#,if variant=="native"{".define(\"TIDKOD_NATIVE\")"}else{""})).unwrap();
    }
    if generated.join("NativeMethods.g.cs").exists() {
        let dotnet = sdk.join("csharp");
        fs::create_dir_all(dotnet.join("Smoke")).unwrap();
        fs::copy(
            repo.join("clients/csharp/README.md"),
            dotnet.join("README.md"),
        )
        .unwrap();
        let project = fs::read_to_string(repo.join("clients/csharp/Tidkod.csproj"))
            .unwrap()
            .replace("../../target/debug/tidkod-generated/native", "../include")
            .replace("../../target/debug", "../lib");
        fs::write(dotnet.join("Tidkod.csproj"), project).unwrap();
        let smoke = fs::read_to_string(repo.join("clients/csharp/Smoke/Smoke.csproj")).unwrap();
        let smoke = if variant == "core" {
            smoke.replace(
                "<OutputType>",
                "<TidkodCoreOnly>true</TidkodCoreOnly><OutputType>",
            )
        } else {
            smoke
        };
        fs::write(dotnet.join("Smoke/Smoke.csproj"), smoke).unwrap();
        fs::copy(
            repo.join("clients/csharp/Smoke/Program.cs"),
            dotnet.join("Smoke/Program.cs"),
        )
        .unwrap();
    }
    // Include the buildable source distribution, with only the native SDK members.
    let source = sdk.join("source");
    fs::create_dir_all(&source).unwrap();
    // Preserve workspace membership so the supplied lockfile remains valid with --locked.
    for member in [
        ".cargo",
        "protocol",
        "native",
        "clients/bindings",
        "clients/wasm",
        "clients/sdk",
    ] {
        copy_tree(&repo.join(member), &source.join(member));
    }
    for file in [
        "Tidkod.csproj",
        "Smoke/Smoke.csproj",
        "Smoke/Program.cs",
        "README.md",
    ] {
        let to = source.join("clients/csharp").join(file);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(repo.join("clients/csharp").join(file), to).unwrap();
    }
    fs::copy(repo.join("Cargo.lock"), source.join("Cargo.lock")).unwrap();
    fs::copy(repo.join("Cargo.toml"), source.join("Cargo.toml")).unwrap();
    for name in ["LICENSE", "LICENSE-MIT", "LICENSE-APACHE"] {
        let path = repo.join(name);
        if path.exists() {
            fs::copy(&path, source.join(name)).unwrap();
            fs::copy(&path, sdk.join(name)).unwrap();
        }
    }
    fs::write(
        sdk.join("BUILD.txt"),
        format!(
            "target={target}\nvariant={variant}\nversion={}\n",
            env::var("CARGO_PKG_VERSION").unwrap()
        ),
    )
    .unwrap();
    if env::var_os("TIDKOD_SDK_SWIFT_DYLIB").is_some() {
        assert!(
            target.contains("apple-darwin"),
            "Swift dylibs currently target macOS"
        );
        swift_dynamic::build(&sdk, variant == "native", &target);
    }
    println!("cargo:warning=SDK written to {}", sdk.display());
}
