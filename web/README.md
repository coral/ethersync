# Browser follower

This is a local portability test application. The native leader and browser use the same
protobuf schema, clock estimator, timeline arithmetic, and correction policy in
[`tidkod-protocol`](../protocol). [`clients/wasm`](../clients/wasm) wraps those APIs with
wasm-bindgen. TypeScript uses published `@moq/net` for WebTransport and MoQ framing; it does
not implement a second synchronization algorithm. No local MoQ checkout dependency is used.

## Run

Requirements: Rust, the `wasm32-unknown-unknown` target, Node >= 22.12, and pnpm 10.28.

```sh
# From repository root, terminal 1:
cargo run -p tidkod --example leader -- --bind 0.0.0.0:4443
# Enter `play` in its command line when ready.

# Terminal 2:
rustup target add wasm32-unknown-unknown
cd web
pnpm install --frozen-lockfile
pnpm dev
```

The build script compiles `clients/wasm`, installs the matching wasm-bindgen CLI into ignored
`web/.tools` if missing, and generates disposable modules in `web/pkg` and `web/pkg-node`.
All web build tooling and the pnpm lockfile live here; Rust binding source lives in `clients/wasm`.

Open Vite's displayed URL (normally `http://127.0.0.1:5173`). Set the address to `127.0.0.1:4443`
for a local leader, or its LAN IP and port. IPv6 addresses need brackets (`[::1]:4443`);
scoped link-local IPv6 URLs are not portable through browser APIs. Paste the leader's displayed
64-character SHA-256 fingerprint and connect. Colons in a copied fingerprint are accepted.

For two computers, run the web development server on the **browser computer** and open its
localhost URL; the leader address can be remote. Serving the page from an unencrypted LAN IP
is not a secure context. Serving production files remotely requires trusted HTTPS. The leader
only needs its existing UDP port; it does not serve this page or need a separate TCP endpoint.

WebTransport offers bidirectional streams and unreliable datagrams over HTTP/3. The adapter
disables WebSocket fallback and requires `moq-lite-05`. Certificate hashes are passed directly
to WebTransport; they are never fetched from an unauthenticated HTTP helper. Exchange pins
through a trusted channel. Pins change when the leader restarts. Browsers cannot use the native
client's address-only skip-verification policy. A stale/wrong pin produces connection failures
and preserves holdover until you enter the new pin and reconnect. The generated certificate
expires roughly 13 days after startup (its 14-day interval starts the day before startup).

## Checks

```sh
pnpm test       # execute compiled WASM in Node with golden/malformed messages and timeline tests
pnpm build      # WASM release build, strict TypeScript check, production bundle
pnpm check     # TypeScript only, after generating WASM
pnpm measure   # real HTTP/3 native-leader vs compiled-WASM same-instant accuracy
# From repository root:
cargo test --workspace
cargo test -p tidkod --test webtransport
```

The HTTP/3 test connects to a real native leader, receives state, exchanges private datagrams,
and rejects a bad certificate pin. It validates the browser-facing server transport, but uses
a Rust client, so it is not evidence of browser interoperability.

Manual browser acceptance:

1. Connect to the native leader. Expect `Connected / Synchronized`; compare diagnostics rather
   than a screenshot's frame difference (renders happen at different instants).
2. Enter `play`, `pause`, `seek 1800`, `shuttle -1 1`, and `shuttle 1 2` on the leader. Check
   the browser's direction, speed, source, and discontinuity fields.
3. Enter `at 1000 900 0 1`; the browser should stop at `00:00:30:00` at 30 fps.
4. Disconnect on the page while running: it should continue in holdover with increasing
   uncertainty. Repeat while paused: the position stays fixed. Reconnect recovers synchronization.
5. Disconnect the network or stop the leader. The page retries only the configured address,
   using capped exponential backoff. After a leader restart, paste the new pin and reconnect.
6. Try a wrong pin and an invalid address. Neither should produce a synchronized new session.
   Open two tabs with the correct pin and check that each acquires independently.
7. Repeat with the `tracked` example, observing degraded health during input loss.
8. Hide/show the tab and resize the window. Check recovery; do not interpret background-tab
   timecode presentation as a timing guarantee.

## Boundaries this experiment exposes

- The protocol crate builds for native and `wasm32-unknown-unknown` without Tokio, Quinn,
  MoQ, mDNS, UUID entropy, or OS-clock dependencies. Probes and read timestamps are caller supplied.
- The binding serializes each reading into a JavaScript object. This allocates; the native
  allocation-free triple-buffer reader remains a separate integration contract.
- Networking, probes, and rendering currently run on the page's main thread. Browser timer
  precision, JavaScript scheduling, GC, suspension, and background throttling can add error or
  interrupt acquisition. Holdover keeps the last trajectory; no browser real-time guarantee is made.
- QUIC RTT/loss statistics are shown as unavailable when the browser doesn't expose them,
  rather than being presented as zero. Clock uncertainty includes the estimator's scheduling
  floor and conservative delay/asymmetry bound, not a guarantee against arbitrary tab suspension.
- WASM validates the 512-byte application-message limit. The JS MoQ library assembles a frame
  before handing it to WASM; this is not an allocation cap on the upstream transport parser.
- The binding uses default fallback and correction settings for now. The Rust protocol API
  supports configurable policies. Browser mDNS and browser leading are not part of this excursion.

Local validation: WASM execution tests, native workspace tests, HTTP/3 integration, TypeScript,
and production bundling are executable here. Live browser testing was blocked because the browser
automation runtime reported no available browser. The manual browser checklist is still outstanding;
no browser accuracy or physical-LAN accuracy claim has been made.

## Same-instant accuracy investigation

`pnpm measure` builds a native fixture in `clients/wasm/examples/accuracy_peer.rs`. The fixture
runs an actual native leader and forwards raw HTTP/3 snapshots/datagrams to the compiled WASM
follower in Node. A separate IPC channel brackets the leader's monotonic clock between the
parent's send/receive times, intersecting 100 intervals to establish a reference offset and
uncertainty **without using the estimator under test or assuming symmetric calibration delay**.
Both timecodes are then evaluated at the same calibrated instant, 600 times at each of
+1x, -1x, +0.5x, +2x, and pause. The script reports signed median, absolute p95/max, clock-mapping
error, remaining slew, and calibration bounds. It fails on excessive error or inconsistent
reference-clock calibration. This exercises real HTTP/3 and actual WASM, with Rust transport
and IPC; it does not measure browser scheduling or screen presentation.

The browser's Connection details now include `Timeline adjustment`: a signed frame offset
from the current mapped trajectory, separately from clock uncertainty. Initial acquisition
no longer preserves startup timing errors with the steady-state slew limiter.

## Timing capture

Reload before connecting to start a bounded replayable capture; capture is enabled by default. Under Connection
details, **Export timing trace** downloads `tidkod-timing.json`. Run `pnpm replay /path/to/file.json`
from `web` to replay the retained prefix through compiled WASM and verify results exactly.
The log preserves the first 16,384 operations and reports later omissions; reload to start again.
Use `?trace=0` to disable capture. The details panel also shows the feasible clock-offset interval
(or inconsistent timing evidence). See [timing APIs](../docs/timing-apis.md) for assumptions,
deadline semantics, timestamp placement, and measurement limitations.

The automatic **Same-computer TOD check** in Connection details compares sampled timecode against
local wall time after `tod` on a leader on this computer. It does not compare screen pixels or
apply a guessed correction. New traces include paired wall-clock references; `pnpm analyze PATH`
summarizes them alongside RTT and remaining slew. Delay compensation is available to adapters via
`read_for_presentation(nowMs, delayMs)`, without an end-user delay input.
