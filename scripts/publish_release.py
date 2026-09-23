#!/usr/bin/env python3
"""Publish only a complete, verified SDK release; never move a Git tag."""
import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import tempfile

from check_release import check
from sdk import TARGETS, machine, run, unpack, version


def expected_assets(release_version):
    names = {f"tidkod-{release_version}-{variant}-{target}" + (".zip" if "windows" in target else ".tar.gz")
             for target in TARGETS for variant in ("native", "core")}
    names.add(f"tidkod-{release_version}-apple-xcframework.zip")
    return names


def validate_assets(directory, release_version):
    assets = sorted(p for p in directory.iterdir() if p.name != "SHA256SUMS")
    expected = expected_assets(release_version)
    assert {p.name for p in assets} == expected, "Incomplete or unexpected release asset set"
    for path in assets:
        with tempfile.TemporaryDirectory(prefix="tidkod-verify-") as temp:
            sdk = unpack(path, Path(temp) / "sdk")
            info = dict(line.split("=", 1) for line in (sdk / "BUILD.txt").read_text().splitlines())
            assert info["version"] == release_version
            for name in ("LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY.json"):
                assert (sdk / name).is_file(), name
            if "apple-xcframework" in path.name:
                assert info["variant"] == "native" and info["profile"] == "release"
                assert (sdk / "Package.swift").is_file()
                framework = sdk / "RustTidkod.xcframework"
                entries = plistlib.loads((framework / "Info.plist").read_bytes())["AvailableLibraries"]
                expected_slices = {
                    "macos-arm64_x86_64": ("macos", ["arm64", "x86_64"], ""),
                    "ios-arm64": ("ios", ["arm64"], ""),
                    "ios-arm64-simulator": ("ios", ["arm64"], "simulator"),
                }
                assert {e["LibraryIdentifier"] for e in entries} == set(expected_slices)
                assert len(entries) == 3
                for entry in entries:
                    platform, architectures, variant = expected_slices[entry["LibraryIdentifier"]]
                    assert entry["SupportedPlatform"] == platform
                    assert sorted(entry["SupportedArchitectures"]) == architectures
                    assert entry.get("SupportedPlatformVariant", "") == variant
                    assert (framework / entry["LibraryIdentifier"] / "libtidkod_bindings.a").is_file()
                continue
            target, variant = info["target"], info["variant"]
            assert f"-{variant}-{target}." in path.name
            assert info["profile"] == "release"
            windows, apple = "windows" in target, "apple" in target
            library = "tidkod_bindings.dll" if windows else "libtidkod_bindings." + ("dylib" if apple else "so")
            assert machine(sdk / "lib" / library) == target.split("-")[0]
            required = ["include/tidkod.h", "include/tidkod-client.hpp", "examples/client.c", "examples/client.cpp",
                        "lib/tidkod_bindings.lib" if windows else "lib/libtidkod_bindings.a"]
            if windows:
                required += ["lib/tidkod_bindings.dll.lib", "csharp/Tidkod.csproj", "include/NativeMethods.g.cs"]
            if apple:
                required += ["Package.swift", "swift-dylib/libTidkod.dylib", "swift-dylib/libTidkodSys.dylib"]
            for name in required:
                assert (sdk / name).is_file(), name
    checksums = directory / "SHA256SUMS"
    checksums.write_text("".join(f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n" for p in assets))
    return [*assets, checksums]


def publish(tag, directory):
    check(tag)
    assets = validate_assets(directory, version())
    # Listing distinguishes absence from authentication/API failures.
    existing = json.loads(run("gh", "api", "--paginate", "--slurp", "repos/{owner}/{repo}/releases", capture=True))
    release = next((r for page in existing for r in page if r["tag_name"] == tag), None)
    if release and not release["draft"]:
        by_name = {a["name"]: a for a in release["assets"]}
        assert set(by_name) == {p.name for p in assets}, "Published release asset set differs; refusing overwrite"
        for path in assets:
            digest = "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()
            assert by_name[path.name].get("digest") == digest, f"Published asset differs: {path.name}"
        print("Release already published with identical assets; nothing changed.")
        return
    if not release:
        run("gh", "release", "create", tag, "--verify-tag", "--draft", "--title", f"Tidkod {tag}", "--generate-notes")
    # A draft is recoverable: replace partial uploads only before publication.
    run("gh", "release", "upload", tag, *assets, "--clobber")
    result = json.loads(run("gh", "release", "view", tag, "--json", "assets", capture=True))
    assert {a["name"] for a in result["assets"]} == {p.name for p in assets}
    run("gh", "release", "edit", tag, "--draft=false", "--prerelease=" + str("-" in tag.split("+", 1)[0]).lower())


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag")
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    publish(args.tag, args.directory.resolve())
