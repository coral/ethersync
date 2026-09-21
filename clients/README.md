# Client bindings

Language and platform bindings live here. They depend on `ethersync-protocol` for parsing,
timecode arithmetic, clock estimation, and timeline synchronization.

- `wasm/`: thin Rust-to-JavaScript follower binding, built with wasm-bindgen.
- Future C/C++/Swift bindings belong beside it; no ABI is reserved yet.

The native Rust client remains in `lib/` (`ethersync`), with leading, following, and discovery.
The browser application and its pnpm build tooling are in `web/`.
