# ethersync-wasm

A thin binding to `ethersync-protocol`, with no dependency on the native `ethersync` crate,
Tokio, MoQ, Quinn, mDNS, or an operating-system clock. It does not create network connections.
See [`web/README.md`](../../web/README.md) for the browser transport and application.

The generated `Follower` exposes:

- `connecting()`, `connected()`, `disconnected()`: transport lifecycle; disconnect preserves holdover.
- `snapshot(bytes, nowMs)`: validate a protobuf snapshot and update the shared timeline state machine.
- `probe(nowMs)`: create and remember a private clock request; send the returned bytes as a MoQ datagram.
- `reply(bytes, nowMs)`: validate/match a clock reply and update the shared estimator.
- `probe_interval_ms()`: 50 ms acquisition, 250 ms synchronized.
- `read(nowMs)`: process scheduled changes/staleness and return evaluated timecode and diagnostics.
- `free()`: release the WASM handle after all adapter tasks have stopped.

Supply monotonic milliseconds from the **same clock epoch** to every call, e.g. `performance.now()`.
Capture receipt time immediately after delivery, before parsing. Negative, non-finite, or out-of-range
timestamps throw. Wire integers remain integers in Rust; JavaScript never parses protobuf timestamps.
The `frames` diagnostic is approximate f64; label generation and timeline arithmetic use Rust Q32.
Returned labels/status objects allocate at the binding boundary; the native allocation-free reader
contract does not apply to JavaScript serialization. Evaluating timecode does not perform networking.

The binding intentionally has a small surface for this portability experiment, with default fallback
and correction policy. The Rust protocol APIs expose customizable policies. This is not a stable C ABI.

`read()` also exposes `mappedLeaderMs` and `correctionFrames` to distinguish clock mapping
from intentional timeline adjustment. The `accuracy_peer` example is a native-only test fixture
for `web/scripts/measure-accuracy.mjs`; its native crates are dev-dependencies, not WASM dependencies.

Additional timing APIs:

- `next_boundary(nowMs)`: next integer frame crossing or scheduled-control wakeup, with local deadline and uncertainty.
- `read(nowMs).offsetEvidence`: feasible clock-offset interval and consistency flag.
- `probe_published(nowMs)`: record completion of publication for the most recently created request.
- `reply_timed(bytes, receivedMs, processedMs)`: retain receipt time while measuring estimator dispatch delay.
- `clock_trace()`: last 128 matched estimator observations, with exact timestamp strings.

See [timing API semantics and trace replay](../../docs/timing-apis.md), including the distinction
between integer crossings and reverse label changes and the limitations of application timestamps.

`read_for_presentation(nowMs, compensationDelayMs)` evaluates the full trajectory at the future
presentation instant without applying scheduled controls early. Positive delay belongs to the
output adapter; it is not additional network compensation. Negative/nonfinite delays and overflow
throw. `read(nowMs)` retains zero-delay behavior and the same result shape.
