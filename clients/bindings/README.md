# Ethersync native bindings

`src/api.rs` is the single foreign API definition and implementation. `build.rs`
parses it with `syn`, rejects unsupported types, generates the adapters, and runs
`cbindgen`, `cxx-build`, `swift-bridge-build`, and `csbindgen`. No Python or Node is involved.
Generated files live beside the Cargo library under
`ethersync-generated/native` or `ethersync-generated/core`. `API.txt` lists the
available signatures. Generated Swift functions throw on Rust errors; C++ uses
explicit outcome handles and builds with exceptions disabled.

## Application APIs and the sys layer

Normal applications use the generated client layer:

| Language | Application layer | Low-level bridge |
| --- | --- | --- |
| Swift | `import Ethersync`: owned classes, methods, typed statuses, native strings/arrays, errors | `EthersyncSys` |
| C++ | `ethersync-client.hpp`: `ethersync::client` classes and `Result<T>` | `ethersync.hpp` |
| C# | `Ethersync`: disposable classes, typed readings, .NET endpoints, managed exceptions | internal `Ethersync.Sys` P/Invoke |
| C | `ethersync-client.h`: `EsEngine`/`EsLeader` handles and typed result structs | `ethersync.h` |

`wrappers.rs` generates all facade methods from the same Rust signatures as the
bridges. Names such as `engine_follower(&Engine, ...)` become instance methods;
`*_new` becomes a Swift initializer or C++ `create()`. There is no hand-written
per-operation implementation in each language. The Swift layer also translates
status fields to enums and exposes `Reading.timecode`; the numeric ABI stays in
the sys module. Configuration handles are mutable builders, not copyable value
configurations.

```swift
import Ethersync

let engine = try Engine()
let options = try FollowerOptions(endpoint: .loopback(port: 4443))
// options.pin(fingerprint: "leader SHA-256 fingerprint")
let follower = try engine.follower(options: options)
let reader = try follower.reader()

// Call from your UI's display callback; this does no networking.
let reading = reader.read()
if reading.synchronization == .synchronized {
    print(reading.timecode)
}
// Keep engine/follower/reader alive for the application's session.
```

Swift handles are reference types with ARC cleanup. They intentionally do not
conform to `Sendable`: serialize access on one actor or queue, including reads on
a particular reader. Swift error values contain a normal String. Swift `Reading`
is a value snapshot; reading it does not allocate, while formatting `timecode` does.

C++ handles are move-only RAII classes. Fallible methods return `Result<T>`; check
its boolean value before `value()` or `take()`. `error()` is a `std::string`.
No exceptions are needed; misuse of an empty result aborts. String and byte-buffer
results convert to standard C++ containers. C retains explicit ownership: check
`result.status`, free `result.error` when non-null, and dispose owned handles with
`es_<type>_dispose(&handle)`. Disposal clears that handle. Do not copy owning C
handles or dispose both a result's handle and an extracted alias. Buffer results
are length-delimited `EsBuffer` values with the same documented free function.

The runnable `examples/client.swift`, `client.cpp`, and `client.c` exercise these
application APIs. `main.swift` and `smoke.*` remain lower-level ABI tests.

```sh
cargo build -p ethersync-bindings
# Runtime/network-free variant, in a separate artifact directory:
cargo build -p ethersync-bindings --no-default-features --features c,cpp,swift --target-dir target/core
```

The native variant provides engines, generated/tracked leaders, followers,
discovery, transport controls, readers, and configuration. The core variant only
requires ethersync-protocol: supply snapshots, four-timestamp probes, connection
changes, and monotonic nanoseconds. Neither API exposes Tokio, protobuf, or MoQ.
Core objects never start threads. A native engine owns one socket worker;
optional mDNS additionally owns its daemon thread. Clients do not start or poll a
runtime. The current pinned upstream MoQ dependency still compiles Tokio utility
code transitively; Ethersync does not construct or enter a Tokio runtime.

## Ownership and calling rules

C functions return 0 on success, 1 on error, 2 on a caught Rust panic. Outputs are
valid only after success. Initialize pointer outputs to NULL. An optional final
error pointer receives an owned UTF-8 `EsBuffer`; free it with
`ethersync_buffer_free`. Buffers are length-delimited, not NUL-terminated.
Use each generated `ethersync_<type>_free` exactly once per successful constructor.
NULL is accepted by free functions. Input spans are borrowed for the call.
Non-null pointers must be valid, correctly typed, and not already freed.

C++ opaque values use `rust::Box` ownership. For fallible operations, check
`Outcome<Type>_ok`, inspect `Outcome<Type>_error` on failure, and call
`Outcome<Type>_take` exactly once on success. Taking a failed/already-taken outcome
is a programming error and aborts; operational errors never require exceptions.
Swift opaque objects use generated ARC wrappers. Do not share mutable handles
between threads. Serialize calls to each handle; separate readers may be used on
separate threads. Leader/follower/discovery handles retain their engine. Explicit
engine shutdown stops all children; it does not invalidate allocated handles.

`Reading.frames` is signed, unwrapped; `subframe` is unsigned Q32 fractional
frames. Never combine them through floating point if exact precision matters.
`reader_read`, `reader_read_at`, `core_read`, and boundary reads return records
without allocating or locking. Configuration, diagnostics, strings, and fallible
C++ outcomes may allocate. All supplied timestamps belong to the engine clock
(or the embedding application's single monotonic domain for core-only usage).
Presentation delay predicts ahead and is never automatically inferred from RTT.

## SDK packaging

Build the bindings first. Packaging consumes completed artifacts and never invokes
Cargo recursively. Use separate Cargo target directories for native/core variants.

```sh
ETHERSYNC_SDK_ARTIFACTS="$PWD/target/debug" ETHERSYNC_SDK_OUT="$PWD/dist" \
  cargo build -p ethersync-sdk
ETHERSYNC_SDK_VARIANT=core ETHERSYNC_SDK_ARTIFACTS="$PWD/target/core/debug" \
  ETHERSYNC_SDK_OUT="$PWD/dist" cargo build -p ethersync-sdk
```

The SDK contains libraries, generated C/C++/Swift sources and headers, CMake
consumption examples, and a buildable Rust source distribution with the pinned
lockfile. From the SDK directory: `cmake -S . -B build`, `cmake --build build`,
`ctest --test-dir build`. Swift: compile `include/SwiftBridgeCore.swift`,
`include/Ethersync.swift`, and `examples/main.swift` using
`-import-objc-header include/BridgingHeader.h`, and link the supplied static
library plus the platform libraries listed in CMakeLists.txt. Add
`-D ETHERSYNC_NATIVE` for the native Swift smoke test.

C/C++ target matrix: macOS arm64/x86_64, Linux x86_64, Windows MSVC x86_64.
Swift initially targets macOS. CI runner results, not configuration files or a
local cross-compile alone, establish support on each target.

On macOS the SDK also contains `Package.swift` and a static XCFramework. Add the
SDK directory as a local Swift package, or run `swift run --package-path SDK_DIR
EthersyncSmoke`. XCFramework metadata is assembled in Rust; packaging does not
require an Xcode installation. Swift compilation still requires Apple's toolchain.
Build distributed macOS libraries with `MACOSX_DEPLOYMENT_TARGET=13.0` to match
the package's minimum OS version.

Status values: connection = disconnected 0, connecting 1, connected 2, shutdown 3;
synchronization = uninitialized 0, acquiring 1, synchronized 2, holdover 3;
source kind = generated 0, tracked 1; health = healthy 0, degraded 1.
`Boundary.valid` distinguishes no prediction from a legitimate zero timestamp.
Boundary kind = frame 0, scheduled change 1. Event kind = none 0, connection 1,
correction 2, observation 3, source health 4, error 5. Correction kind =
discontinuity 1, slew 2, hard resynchronization 3, new session 4.

With clock diagnostics enabled, event kind 6 reports `probe_lateness_ns`: intended
probe deadline to application publication completion, not physical packet departure.
`engine_worker_timing` returns lifetime worker pass counts and scheduling maxima;
these include setup and handshakes. Diagnostics may be dropped when the bounded
queue is full, so drain events promptly when collecting latency distributions.

## Swift dynamic libraries

The SwiftPM package uses a static XCFramework by default. To also compile real
Swift dylibs against the Rust dylib, opt into the Swift compiler during packaging:

```sh
ETHERSYNC_SDK_SWIFT_DYLIB=1 ETHERSYNC_SDK_ARTIFACTS="$PWD/target/debug" \
  ETHERSYNC_SDK_OUT="$PWD/dist" cargo build -p ethersync-sdk
# Run dist/ethersync-native-<target>/swift-dylib/EthersyncSmoke
```

This produces `libEthersync.dylib`, `libEthersyncSys.dylib`, and Swift modules in
`swift-dylib/`, using `lib/libethersync_bindings.dylib`. All three libraries use
`@rpath` install names. Embed/sign all three with your app, configure its runtime
library search path, and make the generated Swift modules and `swift-c` module map
available at compile time. The generated sample resolves its libraries relative
to its executable. Build Swift modules with your app's compiler; this is not a
promise of Swift binary module stability across compiler versions. Rust builds
and default source packaging do not require a Swift compiler.

## Typed IP endpoints

Swift `Endpoint` is an immutable wrapper value. Its Network framework adapters
accept `IPv4Address`, `IPv6Address`, and `NWEndpoint.Port`:

```swift
import Ethersync
import Network

let options = LeaderOptions()
options.bind(to: try .loopback(port: .any)) // OS selects the listen port
let follower = try FollowerOptions(endpoint: .ipv4(.loopback, port: 4443))
let ipv6 = try Endpoint.ipv6(.loopback, port: 4443)
```

A running leader exposes `leader.endpoint()`, so a follower can use its bound
endpoint directly. A wildcard listen address must be replaced with a reachable
IP before connecting. Follower endpoints reject wildcard addresses and port zero.
IPv6 adapters preserve the address's interface scope, or accept an explicit numeric
`scopeID`. `Endpoint.parse(address:)` remains available for text input; it accepts
numeric socket addresses, including `[fe80::1%7]:4443`, but does not perform DNS or
resolve interface names. All languages share Rust's byte-length and port validation.
C++ uses `Endpoint::loopback(0)` and `options.bind_endpoint(endpoint.value())`;
C uses `es_endpoint_loopback(0)` and `es_leader_options_bind_endpoint`.
The small Swift template only adapts Apple platform types to those shared operations.

C# build, ownership, and core-only usage: [C# client guide](../csharp/README.md).
