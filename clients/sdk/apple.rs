//! Aggregate completed Apple static libraries. Never invokes a compiler or Cargo.
use std::{env, ffi::OsStr, fs, path::Path};

pub fn package(artifacts: &OsStr, destination: &Path) {
    let sdk = destination.join("Ethersync");
    let binary = sdk.join("RustEthersync.xcframework");
    fs::create_dir_all(&binary).unwrap();
    let artifacts: Vec<_> = env::split_paths(artifacts).collect();
    let intel = artifacts
        .iter()
        .find(|dir| {
            fs::read_to_string(dir.join("ethersync-generated/native/target.txt"))
                .unwrap()
                .trim()
                == "x86_64-apple-darwin"
        })
        .expect("Intel macOS artifact is required");
    let mut entries = Vec::new();
    let mut reference = None;
    let mut targets = std::collections::BTreeSet::new();
    for dir in &artifacts {
        let generated = dir.join("ethersync-generated/native");
        let target = fs::read_to_string(generated.join("target.txt")).unwrap();
        let target = target.trim();
        assert!(targets.insert(target.to_owned()), "duplicate Apple target");
        let (identifier, platform, variant) = match target {
            "aarch64-apple-darwin" | "x86_64-apple-darwin" => ("macos-arm64_x86_64", "macos", ""),
            "aarch64-apple-ios" => ("ios-arm64", "ios", ""),
            "aarch64-apple-ios-sim" => (
                "ios-arm64-simulator",
                "ios",
                "<key>SupportedPlatformVariant</key><string>simulator</string>",
            ),
            _ => panic!("unsupported Apple aggregate target: {target}"),
        };
        println!("cargo:rerun-if-changed={}", generated.display());
        let source_files = [
            "SwiftBridgeCore.swift",
            "Ethersync.swift",
            "EthersyncClient.swift",
            "SwiftBridgeCore.h",
            "EthersyncSwift.h",
            "BridgingHeader.h",
        ];
        let sources: Vec<_> = source_files
            .iter()
            .map(|name| fs::read(generated.join(name)).unwrap())
            .collect();
        if let Some(ref expected) = reference {
            assert_eq!(
                expected, &sources,
                "generated Apple interfaces differ between targets"
            );
        } else {
            fs::create_dir_all(sdk.join("swift")).unwrap();
            fs::create_dir_all(sdk.join("swift-client")).unwrap();
            for name in ["SwiftBridgeCore.swift", "Ethersync.swift"] {
                let source = fs::read_to_string(generated.join(name)).unwrap();
                fs::write(
                    sdk.join("swift").join(name),
                    format!("import RustEthersync\n{source}"),
                )
                .unwrap();
            }
            fs::copy(
                generated.join("EthersyncClient.swift"),
                sdk.join("swift-client/Ethersync.swift"),
            )
            .unwrap();
            reference = Some(sources);
        }
        if target == "x86_64-apple-darwin" {
            continue;
        }
        let slice = binary.join(identifier);
        let headers = slice.join("Headers");
        fs::create_dir_all(&headers).unwrap();
        for name in ["SwiftBridgeCore.h", "EthersyncSwift.h", "BridgingHeader.h"] {
            fs::copy(generated.join(name), headers.join(name)).unwrap();
        }
        fs::write(
            headers.join("module.modulemap"),
            "module RustEthersync { header \"BridgingHeader.h\" export * }\n",
        )
        .unwrap();
        let library = dir.join("libethersync_bindings.a");
        println!("cargo:rerun-if-changed={}", library.display());
        if platform == "macos" {
            assert!(
                std::process::Command::new("lipo")
                    .arg("-create")
                    .arg(&library)
                    .arg(intel.join("libethersync_bindings.a"))
                    .arg("-output")
                    .arg(slice.join("libethersync_bindings.a"))
                    .status()
                    .unwrap()
                    .success(),
                "lipo failed"
            );
        } else {
            fs::copy(library, slice.join("libethersync_bindings.a")).unwrap();
        }
        let architectures = if platform == "macos" {
            "<string>arm64</string><string>x86_64</string>"
        } else {
            "<string>arm64</string>"
        };
        entries.push(format!("<dict><key>LibraryIdentifier</key><string>{identifier}</string><key>LibraryPath</key><string>libethersync_bindings.a</string><key>HeadersPath</key><string>Headers</string><key>SupportedPlatform</key><string>{platform}</string>{variant}<key>SupportedArchitectures</key><array>{architectures}</array></dict>"));
    }
    assert_eq!(
        targets.len(),
        4,
        "Apple SDK requires both macOS architectures, iOS device, and iOS simulator artifacts"
    );
    fs::write(binary.join("Info.plist"), format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>CFBundlePackageType</key><string>XFWK</string><key>XCFrameworkFormatVersion</key><string>1.0</string><key>AvailableLibraries</key><array>{}</array></dict></plist>", entries.join(""))).unwrap();
    fs::write(sdk.join("Package.swift"), r#"// swift-tools-version: 6.0
import PackageDescription
let package = Package(name: "Ethersync", platforms: [.macOS(.v13), .iOS("26.0")], products: [.library(name: "Ethersync", targets: ["Ethersync"])], targets: [
    .binaryTarget(name: "RustEthersync", path: "RustEthersync.xcframework"),
    .target(name: "EthersyncSys", dependencies: ["RustEthersync"], path: "swift", linkerSettings: [.linkedLibrary("c++"), .linkedFramework("Security"), .linkedFramework("SystemConfiguration"), .linkedFramework("CoreFoundation")]),
    .target(name: "Ethersync", dependencies: ["EthersyncSys"], path: "swift-client")
], swiftLanguageModes: [.v5])
"#).unwrap();
}
