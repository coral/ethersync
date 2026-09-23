# Agent instructions

These instructions apply throughout this repository.

## Protected README files

**Agents are NOT allowed to change README.md files.** This includes the root
`README.md` and README files in subdirectories, regardless of filename casing.
Do not edit, rewrite, format, rename, delete, or regenerate them. Do not let
formatters, generators, or other tools change them indirectly. Preserve any
existing user edits. Put documentation updates in appropriate files under
`docs/` or report proposed README changes to the user without applying them.

## Repository layout

Tidkod provides LAN timecode synchronization with native and browser clients.
The Rust workspace uses edition 2024 and Cargo resolver 3.

- `protocol/` (`tidkod-protocol`): protobuf schema, wire validation, exact
  timecode arithmetic, clock estimation, timelines, tracking, and shared timing
  APIs. Both native and WASM clients use this core.
- `native/` (`tidkod`): public native API, engine worker, discovery,
  networking, leaders, followers, and readers. Its internal `src/transport/`
  module drives QUIC/WebTransport/MoQ explicitly without an executor.
- `native/examples/`: leader, follower, and tracked playback examples.
- `clients/bindings/`: C, C++, Swift, and C# binding generation and examples.
- `clients/csharp/`: .NET project and smoke consumer.
- `clients/sdk/`: native and Apple SDK packaging through Cargo build scripts.
- `clients/wasm/`: WASM bindings to the protocol core.
- `web/`: TypeScript/Vite browser application, WASM build tooling, and tests.
- `docs/`: protocol, timing semantics, validation, and native integration docs.
- `scripts/`: repository checks, SDK packaging, documentation generation, and
  measurement utilities.

## Implementation constraints

- Keep shared timing and protocol behavior in `protocol/` so native and browser
  clients remain consistent. Read `docs/protocol.md` and `docs/timing-apis.md`
  before changing wire formats, clock behavior, or timecode semantics.
- Preserve exact rational rates and signed Q32 positions, including paused and
  reverse playback, discontinuities, scheduled changes, and holdover behavior.
- Keep native reader evaluation bounded and free of allocations, locks, sleeps,
  and network operations. Preserve bounded queues and nonblocking diagnostics.
- Treat clock uncertainty, timeline correction, and presentation latency as
  separate quantities. Do not claim physical-network or display accuracy from
  simulation results or estimator diagnostics alone.
- `protocol/proto/tidkod/v1/tidkod.proto` is the schema source. Preserve
  compatibility and golden fixtures unless a protocol change is intentional.
- `clients/bindings/src/api.rs` defines the foreign API. Update the generators
  (`build.rs`, `wrappers.rs`, `cpp.rs`, `csharp.rs`) and templates when needed; do not patch
  generated bindings as the implementation of a fix.
- Preserve both native and core-only binding variants and foreign-language
  ownership/error handling. Use separate Cargo target directories for variants.
- Keep changes focused and preserve unrelated working-tree edits. Do not commit
  or rewrite Git history unless the user requests it.

## Generated files and dependencies

- Cargo output and generated bindings belong under `target/`, including
  `tidkod-generated/native` and `tidkod-generated/core` beside artifacts.
- SDK packages normally go under `dist/`. WASM glue goes to `web/pkg/` and
  `web/pkg-node/`; the local wasm-bindgen tool goes to `web/.tools/`.
- Respect `.gitignore` and `web/.gitignore`. Do not force-add build artifacts,
  native libraries, caches, or generated SDK packages.
- `docs/messages.md` is intentionally tracked generated documentation. Regenerate
  it with `python3 scripts/generate_docs.py` after relevant schema changes.
- Keep `Cargo.lock` and `web/pnpm-lock.yaml` tracked. Use `--locked` for routine
  Cargo checks and `pnpm --dir web install --frozen-lockfile` for web dependencies.
- Match the wasm-bindgen CLI to the exact crate version pinned in
  `clients/wasm/Cargo.toml`; `web/scripts/build-wasm.mjs` manages this tool.
- Project code is licensed `MIT OR Apache-2.0`. Preserve `LICENSE-MIT`,
  `LICENSE-APACHE`, and third-party notices in `native/licenses/`. Check new
  dependencies' licenses; do not assume the project license replaces theirs.

## Validation

Run commands from the repository root unless stated otherwise. Choose checks
appropriate to the change; documentation-only changes do not require builds.
Report what ran and any missing tools, platform limitations, or failures.

For Rust changes, the standard checks are:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

`bash scripts/check.sh` is the broader validation entry point. It also checks
Rustdoc, Buf lint/format/build, generated schema documentation, and example
smoke tests. It requires Buf and protoc-gen-doc in addition to Rust and Python.
It generates schema documentation in temporary output and checks `docs/messages.md`.

For web or WASM changes, run the relevant checks:

```sh
pnpm --dir web check
pnpm --dir web test
pnpm --dir web build
```

Tests and builds regenerate WASM and require the `wasm32-unknown-unknown` Rust
target. The build script may install its pinned wasm-bindgen CLI.

For binding changes, build `tidkod-bindings` and exercise the affected
language consumers. Use `.github/workflows/native.yml` as the reference for
native/core packaging, CMake/CTest, .NET, and macOS Swift checks. Apple packaging
uses `scripts/build-apple-sdk.sh`; keep deployment targets consistent with
`.cargo/config.toml` and the generated Swift package.

The ignored mDNS integration test requires a multicast-capable interface:

```sh
cargo test -p tidkod --locked --test mdns -- --ignored --nocapture
```

Report multicast/network failures separately from direct QUIC results. See
`docs/validation.md` for measurement requirements and multi-machine procedures.
Before finishing, run `git diff --check` and inspect the diff to ensure protected
README files and unrelated user changes were not altered by your work.

## Releases and SDK checks

- Read `docs/releasing.md` before changing packaging or release workflows.
- The Rust package and import are `tidkod`; foreign library filenames
  remain `tidkod_bindings`. C, C++, and C# share its exported C ABI.
- Only `tidkod-protocol` and `tidkod` may be published to crates.io.
  Registry publishing is gated off in `release.toml` pending upstream MoQ.
- Use `scripts/sdk.py` to build and test archived native/core SDKs. Consumer
  checks must exercise optimized shared libraries as well as static libraries.
- Release smoke checks must execute even with NDEBUG defined. Never put calls
  needed for test execution exclusively inside C/C++ assert expressions.
- Do not publish a release with missing target/variant artifacts or failed tests.
