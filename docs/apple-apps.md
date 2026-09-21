# Apple Swift SDK

Build an arm64 Swift package containing macOS, iOS-device, and iOS-simulator static libraries:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
./scripts/build-apple-sdk.sh
```

Requires Apple's Xcode SDKs. `ETHERSYNC_SDK_OUT` selects the output parent (default `dist/apple`); the package is named `Ethersync`. `CARGO_TARGET_DIR` selects intermediate output and `ETHERSYNC_BUILD_PROFILE=debug` selects debug libraries. Release is the default. Minimum versions are macOS 13 for the SDK library and iOS 26. App deployment targets can be higher.

`clients/sdk/apple.rs` consumes the three completed artifact directories supplied in `ETHERSYNC_SDK_APPLE_ARTIFACTS`, using the platform's path-list separator. It checks that all generated interfaces agree before assembling the static XCFramework. Packaging never recursively invokes Cargo. Existing single-target SDK packaging is unchanged.

The generated `TimecodeFormat` class is available in native and core-only variants:

```swift
let format = try TimecodeFormat(numerator: 30000, denominator: 1001, dropFrame: true)
let position = try format.position(hours: 0, minutes: 1, seconds: 0, frames: 2)
let elapsed = format.elapsed(nanoseconds: 1_001_000_000) // exactly 30 frames
```

Both methods return `FramePosition` with signed `frames` and unsigned Q32 `subframe`. Label validation and elapsed-time conversion are delegated to `ethersync-protocol`; fractional elapsed-time labels need not match wall-clock labels. Handles retain the bindings' existing serialized-access ownership rules.

Apple applications can keep native Rust QUIC while supplying platform Bonjour. Set leader advertisement to false, do not create native Discovery, and register `_ethersync._udp` in `local.` using the actual bound port, configured identity/name, protocol version `1`, leader fingerprint, and transport `moq-lite-05`. Resolve numeric IP endpoints (preserving IPv6 scopes) and pin the discovered certificate in FollowerOptions. Declare Bonjour services and the local-network usage purpose in the app's Info.plist. Raw multicast entitlements are not needed when using system Bonjour for discovery and unicast QUIC for transport.
