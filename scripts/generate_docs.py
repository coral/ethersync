#!/usr/bin/env python3
"""Generate schema documentation, or check it without touching tracked files."""
import argparse
import json
import pathlib
import subprocess
import tempfile

root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
with tempfile.TemporaryDirectory(prefix="tidkod-schema-") as output:
    template = {"version": "v2", "plugins": [{"local": "protoc-gen-doc", "out": output, "opt": "markdown,messages.md"}]}
    subprocess.run(["buf", "generate", "--template", json.dumps(template)], cwd=root, check=True)
    generated = pathlib.Path(output, "messages.md").read_text()
    text = "\n".join(line.rstrip() for line in generated.splitlines()).rstrip() + "\n"
    destination = root / "docs/messages.md"
    if args.check:
        if destination.read_text() != text:
            raise SystemExit("docs/messages.md is stale; run python3 scripts/generate_docs.py")
    else:
        destination.write_text(text)
