#!/usr/bin/env python3
"""Package the universal Apple SDK and test its archived XCFramework on iOS."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile

from sdk import ROOT, archive, notices, run, unpack, version


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--simulator-major", type=int, default=26)
    args = parser.parse_args()
    if args.simulator_major < 26:
        parser.error("The SDK requires iOS 26 or later")
    with tempfile.TemporaryDirectory(prefix="ethersync-apple-") as temporary:
        work = Path(temporary)
        env = dict(os.environ, ETHERSYNC_SDK_OUT=str(work / "package"), ETHERSYNC_BUILD_PROFILE="release")
        run("bash", "scripts/build-apple-sdk.sh", env=env)
        sdk = work / "package/Ethersync"
        for name in ("LICENSE-MIT", "LICENSE-APACHE"):
            shutil.copyfile(ROOT / name, sdk / name)
        # Include the adapted transport sources/notices referenced by the inventory.
        shutil.copytree(ROOT / "native/src/transport", sdk / "source/native/src/transport")
        shutil.copytree(ROOT / "native/licenses", sdk / "source/native/licenses")
        notices(sdk, "aarch64-apple-ios", "native")
        (sdk / "BUILD.txt").write_text(f"version={version()}\nvariant=native\nprofile=release\nmacos=13.0\nios=26.0\n")
        archive_path = archive(sdk, ROOT / "dist/releases", f"ethersync-{version()}-apple-xcframework", windows=True)
        extracted = unpack(archive_path, work / "extracted")
        info = plistlib.loads((extracted / "RustEthersync.xcframework/Info.plist").read_bytes())
        assert {i["LibraryIdentifier"] for i in info["AvailableLibraries"]} == {
            "macos-arm64_x86_64", "ios-arm64", "ios-arm64-simulator"}
        architectures = run("xcrun", "lipo", "-archs", extracted / "RustEthersync.xcframework/macos-arm64_x86_64/libethersync_bindings.a", capture=True)
        assert set(architectures.split()) == {"arm64", "x86_64"}
        project = work / "consumer"
        shutil.copytree(ROOT / "clients/sdk/ios-smoke", project)
        shutil.copytree(extracted, project / "SDK")
        run("xcodegen", "generate", cwd=project)
        devices = json.loads(run("xcrun", "simctl", "list", "devices", "available", "-j", capture=True))
        candidates = [(runtime, d) for runtime, group in devices["devices"].items()
                      if f".iOS-{args.simulator_major}-" in runtime for d in group if d.get("isAvailable") and d["name"].startswith("iPhone")]
        if not candidates:
            raise RuntimeError(f"An iOS {args.simulator_major} iPhone simulator is required")
        runtime, prototype = candidates[0]
        device = run("xcrun", "simctl", "create", work.name, prototype["deviceTypeIdentifier"], runtime, capture=True).strip()
        try:
            run("xcrun", "simctl", "bootstatus", device, "-b", timeout=180)
            run("xcodebuild", "test", "-project", project / "EthersyncSmoke.xcodeproj",
                "-scheme", "EthersyncSmoke", "-configuration", "Release",
                "-destination", f"platform=iOS Simulator,id={device}",
                "-derivedDataPath", work / "derived", "-resultBundlePath", ROOT / "target" / ("ios-smoke-" + work.name + ".xcresult"),
                "CODE_SIGNING_ALLOWED=NO", "ARCHS=arm64", "ONLY_ACTIVE_ARCH=YES",
                "-parallel-testing-enabled", "NO", timeout=600)
        finally:
            subprocess.run(["xcrun", "simctl", "shutdown", device], check=False)
            subprocess.run(["xcrun", "simctl", "delete", device], check=False)
        print(f"Verified Apple SDK: {archive_path}")


if __name__ == "__main__":
    main()
