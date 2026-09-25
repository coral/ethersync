# Tight alignment and output timing

Tidkod's default correction now targets 250 ms settling rather than removing only
0.03 frames per second (1 ms per second at 30 fps). Corrections remain continuous,
limited to 10% of playback velocity. Pause, reverse, scheduled controls and confirmed
large-error resynchronization retain their existing semantics. Paused residual error
stays pending and cannot claim alignment. `CorrectionPolicy::legacy()` and WASM
`Follower.legacy()` retain the old policy; foreign explicit fixed-slew configuration
continues selecting that policy. Default native/core/WASM constructors use precision
correction. Native and WASM probe at 50 ms while correcting, uncertain, or collecting
recovery evidence; they return to 250 ms when aligned. Core-only transports can call
`Core.probeIntervalNs(nowNs)` instead of implementing their own cadence policy.

`aligned` is an estimated 1 ms timeline-error assessment, not just clock acquisition.
`alignment_error_ns` (WASM `alignmentErrorMs`) includes clock uncertainty propagated
through playback speed and remaining correction. It excludes source timestamp error
and physical output latency. The existing `Synchronized` enum continues describing
acquisition. `resync_generation` / `resyncGeneration` exposes hard realignments even
when the bounded event queue loses notifications. Frozen snapshots retain their
captured lifecycle state: refresh each output block, not just at startup.

These APIs preserve wire compatibility. The foreign Reading ABI has grown: rebuild
consumers and libraries together. `CORE_BUILD_ID`, foreign `Core.buildId()` and WASM
`core_build_id()` identify the protocol-core sources and version. This deterministic
source fingerprint is not a security hash or an identity for the entire application.
Browser captures include it; replay refuses an explicitly different core build.

## Output contract

Use the time an output is intended to become visible/audible, not the time its
callback happened to run. Existing `read_at` and presentation reads evaluate the
complete clock mapping, trajectory, correction and retained controls at that time.
Add known device latency only when it is not already part of the supplied timestamp.
Do not add network RTT or snapshot age.

`ClockBridge` pairs an external host-clock sample with a bracket of engine times.
Conversion returns engine nanoseconds and bracket uncertainty. It applies only to
same-rate host clock domains; it does not estimate an independent audio oscillator.
Refresh the bridge after suspend/domain changes. Never bridge wall time into an
output callback or assume an arbitrary device clock has the host oscillator's rate.

The generated native Apple SDK supplies `PresentationReader`:

```swift
// Set up off the output callback, using a dedicated reader handle.
let output = try PresentationReader(engine: engine, reader: follower.reader())
// Inside CADisplayLink's callback:
let reading = try output.read(atHostTime: link.targetTimestamp)
// For CoreAudio, use the device's mHostTime and only outstanding device latency:
let audioReading = try output.read(atHostTicks: hostTicks, additionalLatencyNs: latency)
```

Own the helper on one execution context; do not share its Reader with a control
actor. Native evaluation performs no allocation, locks, networking or actor hop.
Apple bridge uncertainty is available as `clockUncertaintyNs`; it is separate from
the returned reading's core error assessment. Swift's display target is a platform
prediction, not a measurement of the photons emitted by the monitor.

`TimecodeSnapshot::evaluate_sample(origin_ns, sample_index, sample_rate)` and foreign
`readSample` evaluate samples from one immutable trajectory. WASM exposes
`read_sample(originMs, sampleIndexBigInt, sampleRate)`. Use a cumulative sample index
with a stable origin; do not repeatedly add rounded buffer durations. For independent
device clocks, refresh the origin from the device's actual host timestamp each block.
Scheduled changes are evaluated per sample. This is the scheduling foundation for
LTC, not an LTC encoder or a claim of sample-aligned physical audio devices.

The browser's WASM `PresentationClock` estimates the next refresh from a bounded
history of rAF timestamps, resets on suspension, and reports missed predictions.
It never claims a measured compositor deadline. The web example uses this estimate
for displayed labels and keeps its independent TOD comparison at the actual sampled
instant. The terminal continues using frame-boundary wakeups; terminal/compositor
latency is not known and receives no invented fixed offset.

## Reproducible acceptance measurements

```sh
# Exact common Instant, separate native engines/workers, actual tracked wall samples:
cargo run --release -p tidkod --locked --example accuracy -- --tracked
# Actual Chromium WebTransport, compiled WASM, independent reference pipe:
pnpm --dir web measure:browser --tracked
# Generated playback through the same browser path:
pnpm --dir web measure:browser
```

Defaults are five minutes warmup followed by ten minutes measurement. Both commands
accept `--warmup-seconds N --seconds N` for smoke checks. The browser runner uses an
isolated temporary profile and requires Chromium (`CHROME_PATH` overrides the
platform default). It never uses an existing user profile. Browser/server core IDs
must match; development reloads are disabled during measurement. The reference
channel calibrates independently of the Tidkod estimator, verifies its stability,
and includes reference uncertainty in the browser acceptance decision.

Local acceptance is absolute p99 <= 1 ms and maximum <= 2 ms. Preserve signed median,
clock error, correction debt and sample count. Do not silently discard excursions.
The Swift application adds `trackedSourceSamePresentationInstant`, using its actual
TOD source and presentation reader; `TIDKOD_ACCURACY_WARMUP=300` and
`TIDKOD_ACCURACY_SECONDS=600` select the long run.

Physical wired-LAN targets remain p99 <= 2 ms and maximum <= 5 ms, with independently
calibrated host clocks as specified in `validation.md`. Wi-Fi and screen-transition
measurements must be reported separately. Headless Chromium exercises browser
scheduling and transport, not display scanout. No engine test establishes that
different physical screens, terminal emulators, or DACs present simultaneously.

## Regression evidence

Before the change, a simulated steady-state 5 ms trajectory refinement retained
4 ms after one second despite continuing exchanges and heartbeats. The regression
now requires ±5/20/30 ms errors to fall below 1 ms within one second at both +1x and
-1x while preserving direction and continuity. Separate tests cover pending paused
correction, persistent resync generations, quarantine cadence, degraded source,
contradictory evidence, holdover, sample-phase arithmetic and timestamp overflow.

A queued-observation regression also reproduced a position jump when a newly
computed correction was backdated to the receive timestamp. Corrections now start
at processing time and preserve the existing output position there. The estimator
still receives the original receive timestamp; processing delay is not subtracted
from RTT or misrepresented as network asymmetry.

The original-engine native tracked-TOD baseline on 2026-09-24 completed five minutes
warmup and ten minutes measurement with 99,185 samples per follower. P99 errors were
0.076/0.065 ms; maxima were 0.096/0.098 ms. This baseline did not reproduce the
photographed frame-sized offset. Faster correction fixes the reproduced recovery
weakness; it must not be presented as proof of the photograph's cause.

## Measurements from this implementation

Measured on the development Apple Silicon Mac, with concurrent builds and tests.
These are loopback measurements, not physical LAN or display measurements.

| Path | Warmup / measured | Samples | Absolute p99 | Absolute maximum |
| --- | --- | --- | --- | --- |
| Precision native tracked TOD, two followers | 300 / 600 s | 99,451 each | 0.070 / 0.080 ms | 0.457 / 0.448 ms |
| Chromium WebTransport, tracked TOD | 300 / 600 s | 47,005 | 0.119 ms | 0.145 ms |
| Chromium WebTransport, tracked TOD, build `b9aa875096b07371` | 300 / 600 s | 45,681 | 0.171 ms | 0.213 ms |
| Chromium WebTransport, generated, build `b9aa875096b07371` | 300 / 600 s | 45,689 | 0.173 ms | 0.202 ms |
| Final core native tracked TOD, two followers | 5 / 60 s | 9,922 each | 0.041 / 0.043 ms | 0.047 / 0.049 ms |
| Final core Chromium WebTransport, tracked TOD | 5 / 60 s | 4,644 | 0.079 ms | 0.081 ms |
| Final core Swift app tracked TOD, corrected same-instant dispatch | 5 / 600 s | 50,734 | 0.111 ms | 0.313 ms |

Browser rows have additional independent reference uncertainty: 0.083 ms for the
first long run and 0.103 ms for the final-core short run. Both pass with that
uncertainty added. Final core identity is `0.1.1/e8f1ee83eac91f05`. The first long
runs preceded the queued-observation continuity fix; the final-core runs include
it, alongside its deterministic regression. Do not merge results from different
builds into a single distribution.

The additional browser runs have 0.071 / 0.085 ms reference uncertainty and both
pass.

Two initial Swift soak attempts failed the 2 ms maximum criterion: 9.094 ms
(p99 0.319 ms, 50,714 samples) and 4.925 ms (p99 0.236 ms, 50,874 samples).
Diagnostics showed error closely matching elapsed time between the reads.
A deterministic fixed-timestamp regression then proved that an async protocol
extension fallback shadowed the actor's synchronous method on concrete calls:
the test was reading successive current instants, despite passing a timestamp.
Protocol-typed app calls did honor it. The fallback was removed and the actor
method made explicitly async; the regression now checks both dispatch paths.
These attempts (including the earlier one-minute Swift run) are invalid as
same-instant alignment measurements, retained here for audit, not discarded as
statistical outliers. Their thresholds were not relaxed.
The corrected ten-minute Swift run passed, including a 60-second, four-thread CPU
load interval. Signed median error was +0.053 ms; no sample exceeded 0.313 ms.
This measures the app's tracked source and host-clock presentation reader, not
SwiftUI rendering or physical display latency. Its warmup was five seconds, not
the five minutes used by the native/browser long runs.

Rust formatting, workspace Clippy/tests, Rustdoc, CLI smoke tests, web typechecks,
16 compiled-WASM tests and the web production build passed. Archived native and
core-only SDK checks passed their optimized C/C++ shared/static consumers and
Swift package/shared-library consumers, including clock bridge and sample APIs.
The updated macOS app builds, and all 22 Swift tests pass against the final SDK.
The broader `scripts/check.sh` stopped at missing Buf; schema checks were not run.
.NET and iOS Rust targets were unavailable, so C# execution and iOS packaging were
not validated. No protobuf schema or dependency was changed.
