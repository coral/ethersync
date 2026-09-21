# Versions, CI, and SDK releases

The project has one workspace version and one `vX.Y.Z` tag per release.
`ethersync-protocol` and `libethersync` are the only crates intended for crates.io.
Rust applications use `libethersync::{Engine, LeaderConfig, ...}`. Foreign library
names remain `ethersync_bindings`; C, C++, and C# share the same native binary.

## Local release command

Install the tested cargo-release version:

```sh
cargo install cargo-release --version 1.1.6 --locked
```

From a clean, up-to-date `master`, preview a release before executing it:

```sh
python3 scripts/check_release.py
cargo release patch --workspace
cargo release patch --workspace --execute
```

Use `minor`, `major`, `rc`, or an explicit SemVer version instead of `patch` when
appropriate. The command updates the shared version, local dependency version
requirements, and lockfile, creates a consolidated commit and annotated tag, and
pushes them to `origin`. It does not modify README files. A dry run performs no
publication, tag creation, or Git push. Do not execute a release merely to test
the setup.

**Registry publication is currently disabled in `release.toml`.** Version bumps
and GitHub SDK releases still work. The current MoQ Git pin contains runtime
APIs absent from its published crate, so `libethersync` is not yet publishable.
Do not add a misleading registry fallback for the older same-numbered version.

To enable registry publishing after the upstream release:

1. Replace `workspace.dependencies.moq-core` with the verified crates.io version
   that contains the required APIs, retaining the `moq-net` package alias.
2. Update the lockfile and run the full CI matrix, especially independent MoQ
   interoperability tests. Preserve `moq-lite-05` negotiation.
3. Run `python3 scripts/check_release.py --require-publishable` to reject any
   remaining Git-only or unpublished local runtime/build dependencies.
4. Verify package contents and builds with Cargo packaging/dry runs. The native
   crate's protocol dependency must resolve to the matching published version;
   the initial publication therefore publishes protocol first, then native.
   Both packages contain their own license texts, and native contains its examples
   and internal transport source.
5. Set `publish = true` in `release.toml`, authenticate locally with crates.io,
   and preview cargo-release. Only those two packages are publishable; every
   tooling/bindings crate has `publish = false`. cargo-release publishes in
   dependency order before tagging and pushing.

The names were unregistered when this configuration was prepared; availability
is not a reservation. Registry publication uses the maintainer's local Cargo
credentials, not a GitHub Actions token.

## Platform matrix

| Platform | Architectures | Baseline | Consumer tests |
| --- | --- | --- | --- |
| Windows MSVC | x64, ARM64 | Hosted Windows runners | C/C++ shared and static, C# DLL |
| Linux GNU | x64, ARM64 | Ubuntu 22.04, glibc 2.35 | C/C++ shared and static; also run on Ubuntu 24.04 |
| macOS | Intel, Apple Silicon | macOS 13 | C/C++ shared and static, Swift package and dylibs |
| iOS | ARM64 device and ARM64 simulator | iOS 26 | Device build; Swift XCTest consumer on simulator |

Each desktop target produces separate `native` and `core` SDKs. Native includes
network engines; core includes the protocol-only client. Each contains both
static and dynamic libraries, generated binding sources, consumer examples,
build metadata, licenses, dependency source links, and a buildable Rust source
snapshot. Do not mix headers or libraries between variants or versions.

Windows packages include `ethersync_bindings.dll`, its `.dll.lib` import library,
and the distinct static `ethersync_bindings.lib`. C# uses P/Invoke against the DLL;
C/C++ can choose either linkage. There is no NuGet publication in this pipeline.

macOS packages include `libethersync_bindings.dylib` and `.a`, Swift sources, and
the `libEthersyncSys.dylib`/`libEthersync.dylib` wrapper layers. Compiled Swift
modules require a compatible Swift toolchain; rebuild the provided sources or
use the Swift package when integrating with another toolchain. The universal
Apple ZIP combines both macOS architectures and iOS device/simulator slices in
an XCFramework and includes a source Swift package. iOS uses static linking.

The baseline describes the deployment target, not a claim that every OS version
has been tested. Hosted runner OS/toolchain details are recorded in CI logs.
Artifacts are unsigned and not notarized; application distributors handle their
own signing. Android, musl Linux, x86 Windows, and Intel iOS simulators are outside
this release matrix.

## Build and validate locally

For a desktop target matching the host architecture:

```sh
python3 scripts/sdk.py build --target aarch64-apple-darwin --variant native
python3 scripts/sdk.py build --target aarch64-apple-darwin --variant core
python3 scripts/sdk.py test dist/releases/ethersync-0.1.0-native-aarch64-apple-darwin.tar.gz
```

Replace the example version and target as needed. The build defaults to release;
`--profile debug` is available for iteration but debug archives cannot be released.
The script requires Python 3.12+, Cargo, and CMake, plus .NET 8 on Windows or Swift
on macOS. Windows shared-library inspection uses `dumpbin` from a Visual Studio
developer environment. Install the matching Rust target before building.

Archives are extracted outside the checkout. C/C++ examples compile in Release
with always-active checks; they test both shared and static linkage. Dynamic
dependencies and architecture are inspected. Native clients synchronize a pinned
loopback follower with discovery disabled, avoiding multicast and fixed-port
requirements. C# and Swift exercise their packaged wrappers. Extracted Rust
sources are checked independently with the supplied lockfile.

For the Apple aggregate, install Xcode, XcodeGen, an iOS 26 simulator, and Rust
targets `aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-apple-ios`, and
`aarch64-apple-ios-sim`, then run `python3 scripts/apple_sdk.py`. It builds the
archive and runs the simulator test app against an extracted copy. Simulator
results go under `target/`. For local checks against a newer installed simulator,
pass `--simulator-major 27`; CI keeps the default iOS 26 check. Each run creates
and deletes its own simulator.

## GitHub release and recovery

Pull requests and pushes to `master` run the same validation and SDK matrix used
by version tags. The web application is built/tested but is not a release asset.
Rust CI is pinned to 1.98.1; macOS/iOS consumers use each runner's Xcode.

A tag push validates the tag/version match, runs all checks, and uploads tested
SDKs as Actions artifacts. The publication job requires exactly 12 desktop
archives plus the Apple ZIP, validates their metadata and binary architectures,
and creates `SHA256SUMS`. Names are:

```text
ethersync-{version}-{native|core}-{rust-target}.zip       # Windows
ethersync-{version}-{native|core}-{rust-target}.tar.gz    # macOS/Linux
ethersync-{version}-apple-xcframework.zip
SHA256SUMS
```

Only the final publication job has `contents: write`. It uploads everything to
a draft, verifies the asset list, then publishes it. Prerelease tags create
prereleases. A failed build produces no published release.

For a transient failure, rerun failed jobs on the original tag workflow. Partial
draft uploads can be replaced on that rerun. An already published release is
never overwritten: identical assets are a no-op, and differing assets cause an
error. If code changes are needed, use a new version/tag rather than moving the
old tag. A crates.io publication cannot be rolled back; if an upstream publish
succeeds and a later step fails, use cargo-release's individual recovery steps
after inspecting the registry and local commit/tag state.

README files remain owner-maintained and may contain historical commands. The
commands in this document and the workflow definitions describe this pipeline.
