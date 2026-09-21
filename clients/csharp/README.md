# C# client

.NET 8 or later. `csbindgen` generates the internal `Ethersync.Sys` P/Invoke layer
from the existing Rust C ABI. The shared API schema generates the public owned
classes and value snapshots. The small C# support layer handles safe ownership,
errors, native buffers, and .NET platform types; it does not reimplement syncing.

```csharp
using Ethersync;
using System.Net;

using var engine = new Engine();
using var options = new FollowerOptions(
    new IPEndPoint(IPAddress.Parse("192.168.1.10"), 4443));
// options.Pin("leader SHA-256 fingerprint");
using var follower = engine.Follower(options);
using var reader = follower.Reader();

// Call from your display/update callback; keep the session objects alive.
Reading reading = reader.Read();
if (reading.Synchronization == Synchronization.Synchronized)
    Console.WriteLine(reading.Timecode);
```

Leading uses `LeaderOptions`, `options.Bind(new IPEndPoint(IPAddress.Any, 0))`,
`engine.Leader(options)`, and `leader.Play()/Pause()/Seek(frames, subframe)`.
Discovery, scheduled controls, tracked sources, diagnostics, endpoint scope IDs,
and source/connection status are exposed through the same generated facade.
`TimecodeFormat` provides validated rational rates and drop-frame arithmetic.

## Build and use

```sh
cargo build -p ethersync-bindings
# Or just the native C# bridge (no C++/Swift runtime dependency):
cargo build -p ethersync-bindings --no-default-features --features native,csharp

dotnet run --project clients/csharp/Smoke -c Release
```

Reference `clients/csharp/Ethersync.csproj` in your app. It compiles the generated
sources from `target/debug/ethersync-generated/native` and copies the native
library to the application output directory. No NuGet dependency is required.
Your process architecture must match the native library. For release builds or
custom artifact directories, set absolute MSBuild properties
`EthersyncGeneratedDir` and `EthersyncNativeLibraryDir`.

The SDK packager also includes `csharp/Ethersync.csproj` and `csharp/Smoke` beside
the SDK's generated sources and prebuilt native library. Run:

```sh
ETHERSYNC_SDK_ARTIFACTS="$PWD/target/debug" ETHERSYNC_SDK_OUT="$PWD/dist" cargo build -p ethersync-sdk
dotnet run --project dist/ethersync-native-aarch64-apple-darwin/csharp/Smoke -c Release
```

For a portable core build, use a separate Cargo output directory:

```sh
cargo build -p ethersync-bindings --no-default-features --features csharp --target-dir target/csharp-core
dotnet run --project clients/csharp/Smoke -c Release \
  -p:EthersyncCoreOnly=true \
  -p:EthersyncGeneratedDir="$PWD/target/csharp-core/debug/ethersync-generated/core" \
  -p:EthersyncNativeLibraryDir="$PWD/target/csharp-core/debug"
```

`Core` consumes snapshots/probes supplied by your own transport. Its native library
has no networking/runtime dependency. Do not mix a core assembly with a native
assembly/library; constructors for unavailable native handles are omitted from
the core facade. Build separate distributable SDK directories for each variant.

## Ownership and timing

All owned handles implement `IDisposable` using `SafeHandle`. Prefer `using` for
prompt cleanup; finalization also releases forgotten handles. Child native
leader/follower/discovery handles retain their engine. Explicit `Shutdown` stops
work even when other handles still exist. Calls after disposal throw
`ObjectDisposedException`.

Calls protect the native handle with a `SafeHandle` reference and a nonblocking
atomic exclusivity guard. Concurrent operations on the **same handle** throw
`InvalidOperationException`, preventing unsound overlapping Rust borrows without
blocking the read path. Use one reader per consumer. Disposal may race with a call;
native destruction is deferred until the active call releases its reference;
`Dispose` itself need not wait. Different handles can be used on
different threads. This is not a hard-real-time managed runtime guarantee.

`Read` and `ReadAt` return value snapshots without allocating managed memory.
Formatting `Timecode`, strings, event handles, and returned byte arrays allocate.
Positions retain signed 64-bit whole frames plus unsigned Q32 subframes. Timestamp
arguments are monotonic nanoseconds in the engine's `Now()` domain, not DateTime
or Stopwatch ticks. `ReadForPresentation(nowNs, TimeSpan)` predicts ahead by a
known output delay; it does not estimate screen latency. Invalid native input
throws `EthersyncException` with the native message and status.

The smoke test covers interop errors, bool/layout and drop-frame behavior, exact
positions, allocation-free reads, real pinned loopback following, reconnect,
disposal races, and finalizer reclamation. CI runs native/core consumers across
the desktop native matrix. Local validation is macOS arm64; no Unity, mobile .NET,
NativeAOT, or cross-platform execution is claimed without separate testing.
