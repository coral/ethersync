#!/bin/bash
# Build the native Swift SDK for Apple Silicon and Intel Mac, iPhone/iPad, and arm64 Simulator.
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"
output="${ETHERSYNC_SDK_OUT:-$repo/dist/apple}"
artifacts="${CARGO_TARGET_DIR:-$repo/target/apple}"
# Absolute paths are required by Cargo's SDK build script.
mkdir -p "$output" "$artifacts"
output="$(cd "$output" && pwd)"
artifacts="$(cd "$artifacts" && pwd)"
profile="${ETHERSYNC_BUILD_PROFILE:-release}"
case "$profile" in debug) flags=();; release) flags=(--release);; *) echo 'Use debug or release for ETHERSYNC_BUILD_PROFILE' >&2; exit 1;; esac
for target in aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim; do
    case "$target" in
        *-apple-darwin) sdk=macosx;;
        aarch64-apple-ios) sdk=iphoneos;;
        *) sdk=iphonesimulator;;
    esac
    SDKROOT="$(xcrun --sdk "$sdk" --show-sdk-path)" MACOSX_DEPLOYMENT_TARGET=13.0 IPHONEOS_DEPLOYMENT_TARGET=26.0 \
        cargo build -p ethersync-bindings --locked --target "$target" --target-dir "$artifacts" "${flags[@]}"
done
ETHERSYNC_SDK_OUT="$output" \
ETHERSYNC_SDK_APPLE_ARTIFACTS="$artifacts/aarch64-apple-darwin/$profile:$artifacts/x86_64-apple-darwin/$profile:$artifacts/aarch64-apple-ios/$profile:$artifacts/aarch64-apple-ios-sim/$profile" \
    cargo build -p ethersync-sdk --locked --target-dir "$artifacts"
printf 'Apple Swift package: %s/Ethersync\n' "$output"
