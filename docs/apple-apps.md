# Apple Swift SDK

Build an arm64 Swift package containing macOS, iOS-device, and iOS-simulator static libraries:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
./scripts/build-apple-sdk.sh
```

Requires Apple's Xcode SDKs. `TIDKOD_SDK_OUT` selects the output parent (default `dist/apple`); the package is named `Tidkod`. `CARGO_TARGET_DIR` selects intermediate output and `TIDKOD_BUILD_PROFILE=debug` selects debug libraries. Release is the default. Minimum versions are macOS 13 for the SDK library and iOS 26. App deployment targets can be higher.

`clients/sdk/apple.rs` consumes the three completed artifact directories supplied in `TIDKOD_SDK_APPLE_ARTIFACTS`, using the platform's path-list separator. It checks that all generated interfaces agree before assembling the static XCFramework. Packaging never recursively invokes Cargo. Existing single-target SDK packaging is unchanged.

The generated `TimecodeFormat` class is available in native and core-only variants:

```swift
let format = try TimecodeFormat(numerator: 30000, denominator: 1001, dropFrame: true)
let position = try format.position(hours: 0, minutes: 1, seconds: 0, frames: 2)
let elapsed = format.elapsed(nanoseconds: 1_001_000_000) // exactly 30 frames
```

Both methods return `FramePosition` with signed `frames` and unsigned Q32 `subframe`. Label validation and elapsed-time conversion are delegated to `tidkod-protocol`; fractional elapsed-time labels need not match wall-clock labels. Handles retain the bindings' existing serialized-access ownership rules.

## Automatic system Bonjour

The **native `Tidkod` Swift SDK** includes Apple DNS-SD discovery and advertising.
Rust continues to own QUIC, certificate verification, and synchronization. The
core-only Swift SDK has no network engine or Bonjour dependency. Rust, C, C++, and
C# retain their existing native discovery implementation; `TidkodSys` is the
low-level Rust binding and also retains that behavior.

```swift
let engine = try Engine()
let options = LeaderOptions()
options.name(name: "Studio")
let leader = try engine.leader(options: options) // registers with system Bonjour
let discovery = try engine.discovery()         // browses with system Bonjour

// Poll from your own execution context. Bonjour needs no main run loop.
if discovery.poll() > 0 {
    let options = try FollowerOptions.discovered(
        discovery: discovery, index: 0, addressIndex: 0)
    let follower = try engine.follower(options: options)
    // Retain follower for as long as it is needed.
}
```

Discovery uses `_tidkod._udp` in `local.` exclusively. TXT metadata includes `id`,
`name`, `v=1`, `fp`, and `transport=moq-lite-05`. Discovered follower options pin
the advertised certificate and validate protocol support in Rust. Numeric
endpoints retain IPv6 link-local scope IDs. Repeated sightings across interfaces
merge by identity, fingerprint, and protocol version; equal display names do not
merge distinct leaders. Unsupported versions may be listed but cannot produce
follower options. Malformed or unrelated records are ignored.

`poll()` captures a stable snapshot; getters and follower options use those
indices until the next poll. Serialize access to SDK handles as before. Bonjour
callbacks and cancellation run on a private serial queue, independently of UI
rendering and the Rust worker. Retain `Discovery` to keep browsing, and `Leader`
to keep advertising. Releasing the Swift `Engine` wrapper does not invalidate
retained children. Explicit `engine.shutdown()` stops all its Bonjour operations;
`leader.shutdown()` and `discovery.shutdown()` stop their respective operations.

`leader.advertisementStatus` and `discovery.status` report `.starting`, `.ready`,
`.stopped`, or `.failed(BonjourError)`. A leader with
`options.advertise(enabled: false)` reports `.disabled` and makes no Bonjour
registration. Immediate setup failures throw and clean up partially created
resources. Asynchronous failures are visible through status; a failed
advertisement is withdrawn while the unicast leader remains usable. After
correcting a permission or daemon failure, recreate the discovery/leader handle.
`BonjourError` exposes the operation, numeric DNS-SD code, and
`isPermissionDenied`. Browser `.ready` means browsing was started; leader `.ready`
means the daemon acknowledged its service and address records. Neither means a
remote peer has discovered or synchronized with it.

`LeaderOptions.interface(name:)`, `LeaderOptions.address(address:)`, and the
listener bind constrain publication. Each leader/interface has a unique SRV target with
A/AAAA records matching its actual listener family, bound port, and selected
interfaces; IPv4-only listeners do not accidentally publish system IPv6
addresses. Localhost addresses are local-only. Interface address changes are
reconciled every two seconds. `DiscoveryOptions.interface(name:)` restricts
browsing; `DiscoveryOptions.address(address:)` does not filter browsing, matching
the native API. A missing configured interface throws. When no eligible address
is available, advertisement remains `.starting` until one appears.

For apps migrating from an app-owned Bonjour adapter: remove the separate
registration/browser, stop forcing `advertise(false)`, and use the SDK methods
above. The sibling ethersync-apps application is not modified by this SDK change.
The older `_ethersync._udp` name is not browsed or advertised.

## App permissions and signing

The **host application's** Info.plist needs:

```xml
<key>NSBonjourServices</key>
<array><string>_tidkod._udp</string></array>
<key>NSLocalNetworkUsageDescription</key>
<string>Discover and synchronize timecode with devices on your local network.</string>
```

Local-network access can still be denied by the user. A sandboxed macOS host also
needs outgoing and incoming network permissions for its QUIC clients/listeners
(`com.apple.security.network.client` and `com.apple.security.network.server`).
System Bonjour for the declared service and unicast QUIC do not require the app
to open raw multicast sockets. See Apple's
[local-network privacy guidance](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy).
Validate permission prompts and denial/recovery on a signed physical iOS device;
simulator and macOS tests do not establish device privacy behavior.

Entitlements apply to the executable process, not an embedded dylib. Signing a
Tidkod dylib cannot grant the app local-network access. SDK artifacts remain
unsigned: sign embedded dynamic libraries as part of signing/distributing your
macOS app (normally with the same team), then sign the app. iOS SDK slices are
statically linked into the signed app. There is no SDK-specific entitlement or
reason to disable library validation. See Apple's
[distribution signing guidance](https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac).

## Validation

`python3 scripts/sdk.py build --target aarch64-apple-darwin --variant native`
builds optimized Rust libraries, exercises static/shared C and C++ consumers,
Swift package/dylib consumers, and deterministic Swift Bonjour tests compiled
with the shipped generated adapter. These cover TXT validation, interface-copy
merging, scoped addresses, stable snapshots, permission errors, partial setup
cleanup, and cancellation on the correct queue.

Set `TIDKOD_TEST_BONJOUR=1` for live multicast checks, for example:

```sh
TIDKOD_TEST_BONJOUR=1 python3 scripts/sdk.py test dist/releases/tidkod-0.1.1-native-aarch64-apple-darwin.tar.gz
```

This additionally checks system Bonjour over IPv4/IPv6, child ownership and
withdrawal, then both Swift-to-Rust and Rust-to-Swift discovery with pinned QUIC
synchronization. It requires a multicast-capable interface and local-network
access. Use `TIDKOD_TEST_BONJOUR=apple` to run just the system Bonjour network tests.
If the cross-backend check times out, also run the existing Rust baseline:

```sh
cargo test -p tidkod --locked --test mdns -- --ignored --nocapture
```

Report multicast failures separately from successful direct/system-discovered
QUIC results. These checks do not measure physical clock or display accuracy.
