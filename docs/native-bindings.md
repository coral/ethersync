# Native execution and generated bindings

`protocol` contains the timecode arithmetic, protobuf parsing, clock estimator,
follower correction policy, tracking, and boundary prediction. It has no runtime,
networking, or threads. The existing WASM client and the new core-only native
bindings both use it directly.

The internal `native/src/transport` module owns nonblocking UDP, Quinn's protocol state, TLS, and the
HTTP/3/WebTransport framing. `native/src/worker.rs` is a single `mio::Poll` loop per
engine. It receives bounded synchronous commands and wakes for socket readiness,
commands, protocol notifications, and the earliest actual deadline. It does not
start Tokio, run a general future executor, or create a CPU-sized worker pool.
Optional discovery uses mdns-sd's own daemon thread.

Each socket pass handles at most 32 UDP segments, including GRO segments. Each
connection receives eight successful transport operations and bounded QUIC event
and transmit batches before yielding. Idle unsuccessful polls do not spend the
work budget. QUIC connections are visited in rotating order. Timer registration
and stream progress belong to the owner; readers never access these structures.
Snapshot frames are read incrementally, independently of clock replies. The
former 5 ms maintenance interval and intermediate network-event channel are gone.
Explicit controls publish the local reader snapshot before returning their ack.

The upstream MoQ `Runtime` name describes an injected machine/timer provider.
Our provider stores machines for explicit polling and manages deadlines; it does
not execute submitted futures. Upstream handshake/subscription helpers remain
owned, manually polled futures, and MoQ's internal model has pollable lifecycle
work. This change removes the async *executor*, not every Rust `async` function
inside dependencies. The inspected Git revision is pinned to
`5d0991b9991305be907e6c0682a4e276722eeed0` because its runtime/timer injection API
is newer than the published package with the same version number.

Tokio remains a transitive compiled utility dependency of upstream `web-async`
and `web-transport-proto` (the latter uses I/O traits). Ethersync does not construct
or enter a Tokio runtime; no runtime is required from C, C++, or Swift callers.
Published Tokio-based MoQ libraries are dev dependencies used as independent
interoperability peers. Verify the distinction with:

```sh
cargo tree -p libethersync -e normal -i tokio
cargo tree -p ethersync-bindings --no-default-features --features c,cpp,swift -e normal
```

The second tree contains no Tokio, MoQ, QUIC, TLS, or discovery dependencies.

## Bindings

[The binding guide](../clients/bindings/README.md) documents builds, ownership,
errors, units, and packaging. `clients/bindings/src/api.rs` is the authoritative
foreign surface. The Rust build script parses supported functions and POD
records with `syn`, generates per-language adapters and C++ wrappers, and invokes
cbindgen, csbindgen, and swift-bridge. Unsupported signatures fail the build. No Python/Node is needed.
The adapter intentionally exposes flat functions and opaque owned handles rather
than duplicating the native Rust implementation in each language.

Swift uses generated throwing functions. Its string arguments are owned at the
bridge boundary to avoid swift-bridge 0.1.59's invalid throwing `ToRustStr` closure
generation. C++ uses explicit outcomes and is tested with `-fno-exceptions`.
C uses status returns, output pointers, and owned error buffers; operational
entrypoints catch Rust panics. Precise readings use i64 frames plus u32 subframes,
never a floating-point representation of the entire position.

The SDK packager consumes completed Cargo artifacts; it never recursively builds
Cargo from a build script. It includes generated source, prebuilt libraries,
consumer examples, and a buildable source distribution. Use isolated target
directories for native/core variants. macOS arm64 consumers were executed locally;
other targets require their CI runs. The workflow uses the architectures listed
in [GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

## Local measurements, 2026-09-20

Before replacing the transport, the HTTP/3/WASM IPC measurement acquired in
566 ms. Moving-timecode p95 errors were 0.646, 0.304, 0.134, and 0.450 ms at
+1x, -1x, +0.5x, and +2x respectively. Afterwards, acquisition was 572 ms and
errors were 0.344, 0.420, 0.182, and 0.556 ms. Paused position error was effectively
zero in both runs. These individual runs establish interoperability and comparable
numerical accuracy; they are not a statistical claim that one transport is faster.

Fifteen loopback followers on one engine produced 0.164 ms p95 same-instant
position error over 1,500 comparisons at 30 fps. The existing recovery, controls,
IPv6, pinning, source-loss, allocation, and HTTP/3 tests also pass on the new path.
These are local numerical tests. They do not measure compositor latency, terminal
presentation timing, Wi-Fi accuracy, or physical timecode output.

Local validation also passed workspace tests and doctests, strict Clippy for the
workspace and core-only bindings, mDNS advertisement/withdrawal, all example
smokes, and the web tests/build. Native and core SDKs were consumed by CMake/CTest
(C and exception-disabled C++) and SwiftPM on macOS arm64. The Swift native
consumer exercised pinning, follower acquisition, reconnect, and discovery shutdown.
The packaged Rust source builds with `--locked`. Swift 6 reports upstream
swift-bridge retroactive-conformance warnings; the generated consumers still build
and run. Other matrix targets are configured in CI but were not executed locally.

## Timer and scheduling review

The owner now reads the current timer deadlines after all jobs have been polled;
it does not use a deadline cached before origin/peer polling. Timer rescheduling
wakes registered owners, and timer callbacks run outside both the registry and
timer locks. Tests cover earlier rescheduling, cancellation, expiry, dropped
registrations, and reentrant callbacks. Application receive batches explicitly
wake the owner when their eight-message budget is exhausted, because stopping on
a ready result has not necessarily registered a pending notification. Job order
rotates each pass, and exhausted factory batches schedule another pass.

Enable `FollowerConfig.clock_diagnostics` to receive `Event::ProbeTiming`:
`lateness_ns` is application publication completion minus the intended probe
deadline. It does not measure socket departure, one-way network delay, or screen
presentation. Diagnostics use the existing bounded event queue and can be dropped
by a slow consumer. `Engine::worker_timing()` reports lifetime pass counts, maximum
pass duration, and maximum lateness relative to the previous selected deadline.
Counters are sampled independently; setup and cryptographic handshakes are included.
The generated native bindings expose these as event kind 6 (`probe_lateness_ns`)
and `engine_worker_timing`.

Reproduce the load test with:

```sh
cargo test -p libethersync --test poll_worker -- --nocapture
cargo test -p libethersync --test poll_worker probe_deadline_tails_with_discovery -- --ignored --nocapture
```

The first review run with 15 followers, 100 Hz control changes, and periodic
reconnects recorded 1,067 probe publications: lateness p95 0.957 ms, p99 1.316 ms,
maximum 3.111 ms. The longest worker pass was 20.365 ms, including startup and
handshakes. This is a measured sample, not a hard scheduling bound. Bounded network
operation counts do not bound cryptographic work or OS scheduling latency. The
load test's 500 ms guard detects starvation, not compliance with a real-time SLA.


## Shared-library SDKs

C and C++ use the same exported C ABI as C#. The generated C++ convenience
header is `ethersync-client.hpp`, in namespace `ethersync::client`; it owns C
handles with move-only RAII and exposes explicit `Result<T>` errors. It does not
require CXX or a C++ runtime ABI across the library boundary. Qualify wrapper
classes with that namespace to distinguish them from the opaque C handle types.
The old CXX-specific `ethersync.hpp` API has been removed.

Packaged CMake targets are `Ethersync::shared` and `Ethersync::static`; the
`ethersync` alias selects shared linking. `ETHERSYNC_BUILD_EXAMPLES=OFF` disables
packaged consumer tests when integrating the SDK with `add_subdirectory`.
See [releasing](releasing.md) for the platform matrix and archive validation.
