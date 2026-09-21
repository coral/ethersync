#!/usr/bin/env python3
"""Validate the two-crate publishing contract and optional release tag."""
import argparse
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
SEMVER = r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?"


def check(tag=None, require_publishable=False):
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]
    version = workspace["package"]["version"]
    publishable = set()
    for member in workspace["members"]:
        manifest = tomllib.loads((ROOT / member / "Cargo.toml").read_text())
        package = manifest["package"]
        assert package["version"] == {"workspace": True}, member
        if package.get("publish") is not False:
            publishable.add(package["name"])
            for name in ["LICENSE-MIT", "LICENSE-APACHE"]:
                assert (ROOT / member / name).read_bytes() == (ROOT / name).read_bytes()
    assert publishable == {"ethersync-protocol", "libethersync"}, publishable
    release = tomllib.loads((ROOT / "release.toml").read_text())
    assert release["tag-name"] == "v{{version}}"
    assert release["shared-version"] and release["consolidate-commits"]
    if require_publishable or release["publish"]:
        # Metadata follows transitive Git dependencies as well as direct ones.
        import json
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1", "--locked"], cwd=ROOT))
        nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
        packages = {p["id"]: p for p in metadata["packages"]}
        pending = [p["id"] for p in packages.values() if p["name"] in publishable]
        seen = set()
        while pending:
            ident = pending.pop()
            if ident in seen:
                continue
            seen.add(ident)
            package = packages[ident]
            assert not (package["source"] or "").startswith("git+"), f"Unpublished Git dependency: {ident}"
            assert package["source"] or package["name"] in publishable, f"Unpublished local dependency: {ident}"
            pending.extend(d["pkg"] for d in nodes[ident]["deps"]
                           if any(k["kind"] != "dev" for k in d["dep_kinds"]))
    if tag:
        assert re.fullmatch("v" + SEMVER, tag), "Expected a vX.Y.Z tag (optional prerelease)"
        assert tag == "v" + version, f"Tag {tag} does not match workspace {version}"
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).strip()
        tagged = subprocess.check_output(["git", "rev-parse", f"refs/tags/{tag}^{{commit}}"], cwd=ROOT).strip()
        assert head == tagged, "Checkout is not the tagged commit"
    print(f"Release contract verified: {version}; registry publication {'enabled' if release['publish'] else 'disabled'}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag")
    parser.add_argument("--require-publishable", action="store_true")
    options = parser.parse_args()
    check(options.tag, options.require_publishable)
