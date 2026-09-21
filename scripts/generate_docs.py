#!/usr/bin/env python3
"""Generate schema documentation and normalize protoc-gen-doc's whitespace."""
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
subprocess.run(["buf", "generate"], cwd=root, check=True)
path = root / "docs" / "messages.md"
path.write_text("\n".join(line.rstrip() for line in path.read_text().splitlines()).rstrip() + "\n")
