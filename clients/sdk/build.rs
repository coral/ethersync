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
        if path.is_dir() {
            copy_tree(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(path, to.join(entry.file_name())).unwrap();
        }
    }
}
fn main() {
    for key in [
        "ETHERSYNC_SDK_OUT",
        "ETHERSYNC_SDK_ARTIFACTS",
        "ETHERSYNC_SDK_VARIANT",
        "ETHERSYNC_SDK_SWIFT_DYLIB",
        "ETHERSYNC_SDK_APPLE_ARTIFACTS",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let Some(destination) = env::var_os("ETHERSYNC_SDK_OUT") else {
        return;
    };
    let destination = PathBuf::from(destination);
    if let Some(artifacts) = env::var_os("ETHERSYNC_SDK_APPLE_ARTIFACTS") {
        apple::package(&artifacts, &destination);
        return;
    }
    let artifacts = PathBuf::from(
        env::var_os("ETHERSYNC_SDK_ARTIFACTS")
            .expect("set ETHERSYNC_SDK_ARTIFACTS to the completed Cargo profile directory"),
    );
    let variant = env::var("ETHERSYNC_SDK_VARIANT").unwrap_or_else(|_| "native".into());
    assert!(variant == "native" || variant == "core");
    let generated = artifacts.join("ethersync-generated").join(&variant);
    let target = fs::read_to_string(generated.join("target.txt"))
        .expect("build the requested binding variant first");
    println!("cargo:rerun-if-changed={}", generated.display());
    let sdk = destination.join(format!("ethersync-{variant}-{target}"));
    fs::create_dir_all(sdk.join("lib")).unwrap();
    copy_tree(&generated, &sdk.join("include"));
    let mut copied = false;
    for name in [
        "libethersync_bindings.a",
        "libethersync_bindings.dylib",
        "libethersync_bindings.so",
        "ethersync_bindings.lib",
        "ethersync_bindings.dll",
        "ethersync_bindings.dll.lib",
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
        "lib",
        "app",
        "transport",
        "clients/bindings",
        "clients/csharp",
        "clients/wasm",
        "clients/sdk",
        "examples",
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
    fs::write(sdk.join("CMakeLists.txt"),format!(r#"cmake_minimum_required(VERSION 3.20)
project(EthersyncSDK LANGUAGES C CXX)
add_library(ethersync STATIC IMPORTED GLOBAL)
if(WIN32)
  set_target_properties(ethersync PROPERTIES IMPORTED_LOCATION "${{CMAKE_CURRENT_LIST_DIR}}/lib/ethersync_bindings.lib")
  target_link_libraries(ethersync INTERFACE ws2_32 userenv bcrypt ntdll advapi32 crypt32 secur32)
else()
  set_target_properties(ethersync PROPERTIES IMPORTED_LOCATION "${{CMAKE_CURRENT_LIST_DIR}}/lib/libethersync_bindings.a")
  if(APPLE)
    target_link_libraries(ethersync INTERFACE "-framework Security" "-framework SystemConfiguration" "-framework CoreFoundation" c++)
  else()
    target_link_libraries(ethersync INTERFACE pthread dl m stdc++)
  endif()
endif()
set_target_properties(ethersync PROPERTIES INTERFACE_INCLUDE_DIRECTORIES "${{CMAKE_CURRENT_LIST_DIR}}/include")
add_executable(ethersync-c examples/smoke.c)
target_link_libraries(ethersync-c PRIVATE ethersync)
add_executable(ethersync-cpp examples/smoke.cpp)
target_link_libraries(ethersync-cpp PRIVATE ethersync)
target_compile_features(ethersync-cpp PRIVATE cxx_std_17)
if(MSVC)
  target_compile_options(ethersync-cpp PRIVATE /EHs-c-)
else()
  target_compile_options(ethersync-cpp PRIVATE -fno-exceptions)
endif()
{}
enable_testing()
add_test(NAME c COMMAND ethersync-c)
add_test(NAME cpp COMMAND ethersync-cpp)
add_executable(ethersync-client-c examples/client.c)
add_executable(ethersync-client-cpp examples/client.cpp)
target_link_libraries(ethersync-client-c PRIVATE ethersync)
target_link_libraries(ethersync-client-cpp PRIVATE ethersync)
target_compile_features(ethersync-client-cpp PRIVATE cxx_std_17)
get_target_property(SMOKE_DEFINITIONS ethersync-c COMPILE_DEFINITIONS)
if(SMOKE_DEFINITIONS)
  target_compile_definitions(ethersync-client-c PRIVATE ${{SMOKE_DEFINITIONS}})
  target_compile_definitions(ethersync-client-cpp PRIVATE ${{SMOKE_DEFINITIONS}})
endif()
if(MSVC)
  target_compile_options(ethersync-client-cpp PRIVATE /EHs-c-)
else()
  target_compile_options(ethersync-client-cpp PRIVATE -fno-exceptions)
endif()
add_test(NAME client-c COMMAND ethersync-client-c)
add_test(NAME client-cpp COMMAND ethersync-client-cpp)
"#,if variant=="native"{"target_compile_definitions(ethersync-c PRIVATE ETHERSYNC_NATIVE)\ntarget_compile_definitions(ethersync-cpp PRIVATE ETHERSYNC_NATIVE)"}else{""})).unwrap();
    if target.contains("apple-darwin") && sdk.join("include/Ethersync.swift").exists() {
        let headers = sdk.join("swift-c");
        fs::create_dir_all(&headers).unwrap();
        for name in ["SwiftBridgeCore.h", "EthersyncSwift.h", "BridgingHeader.h"] {
            fs::copy(sdk.join("include").join(name), headers.join(name)).unwrap();
        }
        fs::write(
            headers.join("module.modulemap"),
            "module RustEthersync { header \"BridgingHeader.h\" export * }\n",
        )
        .unwrap();
        let binary = sdk.join("RustEthersync.xcframework");
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
            sdk.join("lib/libethersync_bindings.a"),
            slice.join("libethersync_bindings.a"),
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
<key>LibraryPath</key><string>libethersync_bindings.a</string>
<key>HeadersPath</key><string>Headers</string>
<key>SupportedPlatform</key><string>macos</string>
<key>SupportedArchitectures</key><array><string>{architecture}</string></array>
</dict></array></dict></plist>
"#)).unwrap();
        let swift = sdk.join("swift");
        fs::create_dir_all(&swift).unwrap();
        for name in ["SwiftBridgeCore.swift", "Ethersync.swift"] {
            let text = fs::read_to_string(sdk.join("include").join(name)).unwrap();
            fs::write(swift.join(name), format!("import RustEthersync\n{text}")).unwrap();
        }
        fs::create_dir_all(sdk.join("swift-client")).unwrap();
        fs::copy(
            sdk.join("include/EthersyncClient.swift"),
            sdk.join("swift-client/Ethersync.swift"),
        )
        .unwrap();
        fs::create_dir_all(sdk.join("swift-example")).unwrap();
        let example =
            fs::read_to_string(repo.join("clients/bindings/examples/client.swift")).unwrap();
        fs::write(
            sdk.join("swift-example/main.swift"),
            format!("import Ethersync\n{example}"),
        )
        .unwrap();
        fs::write(sdk.join("Package.swift"),format!(r#"// swift-tools-version: 5.9
import PackageDescription
let package = Package(name: "Ethersync", platforms: [.macOS(.v13)], products: [.library(name: "Ethersync", targets: ["Ethersync"])], targets: [
    .binaryTarget(name: "RustEthersync", path: "RustEthersync.xcframework"),
    .target(name: "EthersyncSys", dependencies: ["RustEthersync"], path: "swift", linkerSettings: [.linkedLibrary("c++"), .linkedFramework("Security"), .linkedFramework("SystemConfiguration"), .linkedFramework("CoreFoundation")]),
    .target(name: "Ethersync", dependencies: ["EthersyncSys"], path: "swift-client"),
    .executableTarget(name: "EthersyncSmoke", dependencies: ["Ethersync"], path: "swift-example", swiftSettings: [{}])
])
"#,if variant=="native"{".define(\"ETHERSYNC_NATIVE\")"}else{""})).unwrap();
    }
    if generated.join("NativeMethods.g.cs").exists() {
        let dotnet = sdk.join("csharp");
        fs::create_dir_all(dotnet.join("Smoke")).unwrap();
        fs::copy(
            repo.join("clients/csharp/README.md"),
            dotnet.join("README.md"),
        )
        .unwrap();
        let project = fs::read_to_string(repo.join("clients/csharp/Ethersync.csproj"))
            .unwrap()
            .replace(
                "../../target/debug/ethersync-generated/native",
                "../include",
            )
            .replace("../../target/debug", "../lib");
        fs::write(dotnet.join("Ethersync.csproj"), project).unwrap();
        let smoke = fs::read_to_string(repo.join("clients/csharp/Smoke/Smoke.csproj")).unwrap();
        let smoke = if variant == "core" {
            smoke.replace(
                "<OutputType>",
                "<EthersyncCoreOnly>true</EthersyncCoreOnly><OutputType>",
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
        "lib",
        "app",
        "transport",
        "clients/bindings",
        "clients/wasm",
        "clients/sdk",
    ] {
        copy_tree(&repo.join(member), &source.join(member));
    }
    for file in [
        "Ethersync.csproj",
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
    // lib registers examples outside its directory.
    copy_tree(&repo.join("examples"), &source.join("examples"));
    for name in ["LICENSE", "LICENSE-MIT", "LICENSE-APACHE"] {
        let path = repo.join(name);
        if path.exists() {
            fs::copy(path, source.join(name)).unwrap();
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
    if env::var_os("ETHERSYNC_SDK_SWIFT_DYLIB").is_some() {
        assert!(
            target.contains("apple-darwin"),
            "Swift dylibs currently target macOS"
        );
        swift_dynamic::build(&sdk, variant == "native", &target);
    }
    println!("cargo:warning=SDK written to {}", sdk.display());
}
